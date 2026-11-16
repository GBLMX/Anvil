// Package fswatch implements Linux directory watches for the workspace gateway.
package fswatch

import (
	"errors"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"
	"unsafe"

	"golang.org/x/sys/unix"
)

const (
	coalesceDelay = 100 * time.Millisecond
	maxBatch      = 1024
	maxPending    = 16384
	defaultMaxDir = 8192
)

var defaultExcludes = map[string]bool{
	".git":         true,
	"target":       true,
	"node_modules": true,
	".cache":       true,
}

// Config describes one watched workspace root.
type Config struct {
	Root       string
	RootRel    string
	Exclude    []string
	MaxDirs    int
	CrossMount bool
}

// Status is the bounded watcher state reported to the editor.
type Status struct {
	Generation    uint64 `json:"generation"`
	Root          string `json:"root"`
	Mode          string `json:"mode"`
	Coverage      string `json:"coverage"`
	WatchedDirs   int    `json:"watched_dirs"`
	SkippedDirs   int    `json:"skipped_dirs"`
	OverflowCount uint64 `json:"overflow_count"`
	LastRescan    int64  `json:"last_rescan"`
	Reason        string `json:"reason,omitempty"`
}

// Event is one coalesced path change.
type Event struct {
	Path  string `json:"path"`
	Kind  string `json:"kind"`
	IsDir bool   `json:"is_dir,omitempty"`
	To    string `json:"to,omitempty"`
}

// Watcher owns one inotify instance and directory watch set.
type Watcher struct {
	mu       sync.Mutex
	fd       int
	root     string
	rootRel  string
	exclude  []string
	maxDirs  int
	cross    bool
	notify   func(string, any)
	status   Status
	wdPath   map[int]string
	pathWD   map[string]int
	pending  map[string]Event
	order    []string
	moved    map[uint32]string
	rescan   bool
	stopped  chan struct{}
	stopOnce sync.Once
	done     chan struct{}
}

// Start creates a watcher and starts the asynchronous initial walk.
func Start(cfg Config, notify func(string, any)) (*Watcher, Status, error) {
	root, err := filepath.Abs(cfg.Root)
	if err != nil {
		return nil, Status{}, err
	}
	info, err := os.Stat(root)
	if err != nil || !info.IsDir() {
		return nil, Status{}, errors.New("watch root must be a directory")
	}
	maxDirs := cfg.MaxDirs
	if maxDirs <= 0 {
		maxDirs = defaultMaxDir
	}
	if notify == nil {
		notify = func(string, any) {}
	}
	fd, err := unix.InotifyInit1(unix.IN_CLOEXEC | unix.IN_NONBLOCK)
	if err != nil {
		return nil, Status{}, err
	}
	watcher := &Watcher{
		fd:      fd,
		root:    root,
		rootRel: cfg.RootRel,
		exclude: cfg.Exclude,
		maxDirs: maxDirs,
		cross:   cfg.CrossMount,
		notify:  notify,
		status: Status{
			Generation: 1,
			Root:       cfg.RootRel,
			Mode:       "native",
			Coverage:   "starting",
		},
		wdPath:  map[int]string{},
		pathWD:  map[string]int{},
		pending: map[string]Event{},
		moved:   map[uint32]string{},
		stopped: make(chan struct{}),
		done:    make(chan struct{}),
	}
	if watcher.addWatch(root, ".") != nil {
		_ = unix.Close(fd)
		return nil, Status{}, errors.New("cannot watch workspace root")
	}
	go watcher.run()
	return watcher, watcher.Status(), nil
}

// Status returns a copy of the current watcher state.
func (w *Watcher) Status() Status {
	w.mu.Lock()
	defer w.mu.Unlock()
	return w.status
}

// Stop tears down the inotify instance and waits for the event loop.
func (w *Watcher) Stop() {
	w.stopOnce.Do(func() { close(w.stopped) })
	<-w.done
}

func (w *Watcher) run() {
	defer close(w.done)
	defer unix.Close(w.fd)
	w.walk(w.root, ".")
	w.publishStatus()
	pollFds := []unix.PollFd{{Fd: int32(w.fd), Events: unix.POLLIN}}
	lastFlush := time.Now()
	for {
		select {
		case <-w.stopped:
			w.mu.Lock()
			w.status.Coverage = "off"
			w.status.Mode = "off"
			w.mu.Unlock()
			w.publishStatus()
			return
		default:
		}
		n, err := unix.Poll(pollFds, 50)
		if err != nil && !errors.Is(err, unix.EINTR) {
			w.markRescan(err.Error())
			continue
		}
		if n > 0 {
			w.readEvents()
		}
		if time.Since(lastFlush) >= coalesceDelay {
			w.flush()
			lastFlush = time.Now()
		}
	}
}

func (w *Watcher) walk(root, rootRel string) {
	rootDev := device(root)
	_ = filepath.WalkDir(root, func(path string, entry fs.DirEntry, walkErr error) error {
		if walkErr != nil {
			w.skipDir()
			if entry != nil && entry.IsDir() {
				return fs.SkipDir
			}
			return nil
		}
		if !entry.IsDir() || path == root {
			return nil
		}
		rel, err := filepath.Rel(root, path)
		if err != nil {
			w.skipDir()
			return fs.SkipDir
		}
		rel = filepath.ToSlash(rel)
		if rootRel != "." {
			rel = filepath.ToSlash(filepath.Join(rootRel, rel))
		}
		if entry.Name() == ".git" {
			_ = w.addWatch(path, rel)
			return fs.SkipDir
		}
		if w.excluded(rel, entry.Name()) {
			return fs.SkipDir
		}
		if !w.cross && rootDev != 0 && device(path) != rootDev {
			w.skipDir()
			return fs.SkipDir
		}
		if len(w.pathWD) >= w.maxDirs {
			w.skipDir()
			w.mu.Lock()
			w.status.Reason = "directory budget reached"
			w.mu.Unlock()
			return fs.SkipDir
		}
		if err := w.addWatch(path, rel); err != nil {
			w.skipDir()
			w.mu.Lock()
			w.status.Reason = err.Error()
			w.mu.Unlock()
			return fs.SkipDir
		}
		return nil
	})
}

func (w *Watcher) excluded(rel, base string) bool {
	if defaultExcludes[base] {
		return true
	}
	for _, excluded := range w.exclude {
		excluded = filepath.ToSlash(strings.TrimSpace(excluded))
		if excluded == "" {
			continue
		}
		if base == excluded || rel == excluded || strings.HasPrefix(rel, excluded+"/") {
			return true
		}
	}
	return false
}

func (w *Watcher) addWatch(path, rel string) error {
	if _, exists := w.pathWD[path]; exists {
		return nil
	}
	mask := uint32(unix.IN_CREATE | unix.IN_DELETE | unix.IN_MODIFY |
		unix.IN_ATTRIB | unix.IN_CLOSE_WRITE | unix.IN_MOVED_FROM |
		unix.IN_MOVED_TO | unix.IN_DELETE_SELF | unix.IN_MOVE_SELF |
		unix.IN_ONLYDIR | unix.IN_DONT_FOLLOW | unix.IN_EXCL_UNLINK)
	wd, err := unix.InotifyAddWatch(w.fd, path, mask)
	if err != nil {
		return err
	}
	w.wdPath[wd] = rel
	w.pathWD[path] = wd
	w.mu.Lock()
	w.status.WatchedDirs = len(w.pathWD)
	w.mu.Unlock()
	return nil
}

func (w *Watcher) skipDir() {
	w.mu.Lock()
	w.status.SkippedDirs++
	w.status.Coverage = "limited"
	w.mu.Unlock()
}

func (w *Watcher) publishStatus() {
	w.mu.Lock()
	status := w.status
	if status.Coverage == "starting" {
		status.Coverage = "full"
	}
	w.status.Coverage = status.Coverage
	w.mu.Unlock()
	w.notify("watch.status", status)
}

func (w *Watcher) readEvents() {
	buffer := make([]byte, 64*1024)
	for {
		n, err := unix.Read(w.fd, buffer)
		if err == unix.EAGAIN || err == unix.EINTR {
			return
		}
		if err != nil || n <= 0 {
			w.markRescan("inotify read failed")
			return
		}
		for offset := 0; offset+unix.SizeofInotifyEvent <= n; {
			event := (*unix.InotifyEvent)(unsafe.Pointer(&buffer[offset]))
			nameStart := offset + unix.SizeofInotifyEvent
			nameEnd := nameStart + int(event.Len)
			if nameEnd > n {
				break
			}
			name := strings.TrimRight(string(buffer[nameStart:nameEnd]), "\x00")
			w.handleEvent(int(event.Wd), event.Mask, event.Cookie, name)
			offset = nameEnd
		}
	}
}

func (w *Watcher) handleEvent(wd int, mask, cookie uint32, name string) {
	if mask&unix.IN_Q_OVERFLOW != 0 {
		w.markRescan("inotify queue overflow")
		return
	}
	if mask&unix.IN_IGNORED != 0 {
		w.dropWatch(wd)
		return
	}
	base, ok := w.wdPath[wd]
	if !ok {
		return
	}
	abs := w.root
	rel := base
	if name != "" {
		abs = filepath.Join(abs, base, name)
		if base == "." {
			rel = name
		} else {
			rel = filepath.Join(base, name)
		}
	}
	rel = filepath.ToSlash(rel)
	isDir := mask&unix.IN_ISDIR != 0
	switch {
	case mask&unix.IN_MOVED_FROM != 0:
		w.mu.Lock()
		w.moved[cookie] = rel
		w.addPending(Event{Path: rel, Kind: "removed", IsDir: isDir})
		w.mu.Unlock()
	case mask&unix.IN_MOVED_TO != 0:
		w.mu.Lock()
		from := w.moved[cookie]
		delete(w.moved, cookie)
		if from != "" {
			delete(w.pending, from)
			w.removeOrder(from)
			w.addPending(Event{Path: from, Kind: "renamed", To: rel, IsDir: isDir})
		} else {
			w.addPending(Event{Path: rel, Kind: "created", IsDir: isDir})
		}
		w.mu.Unlock()
		if isDir {
			_ = w.addWatch(abs, rel)
		}
	case mask&unix.IN_CREATE != 0:
		w.mu.Lock()
		w.addPending(Event{Path: rel, Kind: "created", IsDir: isDir})
		w.mu.Unlock()
		if isDir {
			if err := w.addWatch(abs, rel); err == nil {
				w.walk(abs, rel)
			}
		}
	case mask&unix.IN_DELETE != 0:
		w.mu.Lock()
		w.addPending(Event{Path: rel, Kind: "removed", IsDir: isDir})
		w.mu.Unlock()
	case mask&(unix.IN_MODIFY|unix.IN_CLOSE_WRITE|unix.IN_ATTRIB) != 0:
		w.mu.Lock()
		w.addPending(Event{Path: rel, Kind: "modified", IsDir: isDir})
		w.mu.Unlock()
	case mask&(unix.IN_DELETE_SELF|unix.IN_MOVE_SELF) != 0:
		w.dropWatch(wd)
	}
}

func (w *Watcher) dropWatch(wd int) {
	w.mu.Lock()
	delete(w.wdPath, wd)
	for path, descriptor := range w.pathWD {
		if descriptor == wd {
			delete(w.pathWD, path)
			break
		}
	}
	w.status.WatchedDirs = len(w.pathWD)
	w.mu.Unlock()
}

func (w *Watcher) removeOrder(path string) {
	for index, queued := range w.order {
		if queued == path {
			w.order = append(w.order[:index], w.order[index+1:]...)
			return
		}
	}
}

func (w *Watcher) addPending(event Event) {
	if existing, exists := w.pending[event.Path]; exists {
		if existing.Kind == "removed" || existing.Kind == "renamed" {
			return
		}
		existing.Kind = event.Kind
		existing.IsDir = existing.IsDir || event.IsDir
		if event.To != "" {
			existing.To = event.To
		}
		w.pending[event.Path] = existing
		return
	}
	if len(w.pending) >= maxPending {
		w.rescan = true
		return
	}
	w.pending[event.Path] = event
	w.order = append(w.order, event.Path)
}

func (w *Watcher) flush() {
	w.mu.Lock()
	defer w.mu.Unlock()
	overflow := w.rescan
	if overflow {
		w.pending = map[string]Event{}
		w.order = nil
		w.rescan = false
	}
	if len(w.order) == 0 && !overflow {
		return
	}
	limit := len(w.order)
	if limit > maxBatch {
		limit = maxBatch
	}
	events := make([]Event, 0, limit+1)
	if overflow {
		events = append(events, Event{Kind: "rescan"})
		w.status.OverflowCount++
		w.status.Coverage = "limited"
		w.status.Reason = "event queue overflow"
		w.notify("watch.status", w.status)
	}
	for _, path := range w.order[:limit] {
		if event, exists := w.pending[path]; exists {
			events = append(events, event)
			delete(w.pending, path)
		}
	}
	w.order = w.order[limit:]
	w.notify("watch.events", map[string]any{
		"generation": w.status.Generation,
		"root":       w.status.Root,
		"events":     events,
		"overflow":   overflow,
	})
}

func (w *Watcher) markRescan(reason string) {
	w.mu.Lock()
	w.rescan = true
	w.status.Generation++
	w.status.Coverage = "limited"
	w.status.Reason = reason
	w.status.LastRescan = time.Now().Unix()
	w.mu.Unlock()
	w.notify("watch.status", w.Status())
}

func device(path string) uint64 {
	var stat unix.Stat_t
	if err := unix.Stat(path, &stat); err != nil {
		return 0
	}
	return uint64(stat.Dev)
}
