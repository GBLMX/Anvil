package fswatch

import (
	"golang.org/x/sys/unix"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestWatcherReportsCreatedFile(t *testing.T) {
	root := t.TempDir()
	events := make(chan Event, 16)
	notify := func(method string, params any) {
		if method != "watch.events" {
			return
		}
		payload, ok := params.(map[string]any)
		if !ok {
			return
		}
		batch, ok := payload["events"].([]Event)
		if !ok {
			return
		}
		for _, event := range batch {
			events <- event
		}
	}
	watcher, _, err := Start(Config{Root: root, RootRel: "."}, notify)
	if err != nil {
		t.Fatal(err)
	}
	defer watcher.Stop()

	deadline := time.Now().Add(3 * time.Second)
	for watcher.Status().Coverage != "full" {
		if time.Now().After(deadline) {
			t.Fatalf("watcher did not reach full: %+v", watcher.Status())
		}
		time.Sleep(10 * time.Millisecond)
	}
	if err := os.WriteFile(filepath.Join(root, "new.txt"), []byte("data"), 0o600); err != nil {
		t.Fatal(err)
	}
	deadline = time.Now().Add(3 * time.Second)
	for {
		select {
		case event := <-events:
			if event.Path == "new.txt" &&
				(event.Kind == "created" || event.Kind == "modified") {
				return
			}
		case <-time.After(time.Until(deadline)):
			t.Fatalf("no event for new.txt; status=%+v", watcher.Status())
		}
	}
}

func TestWatcherHonorsDirectoryBudget(t *testing.T) {
	root := t.TempDir()
	for _, name := range []string{"one", "two"} {
		if err := os.Mkdir(filepath.Join(root, name), 0o700); err != nil {
			t.Fatal(err)
		}
	}
	watcher, _, err := Start(Config{Root: root, RootRel: ".", MaxDirs: 1}, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer watcher.Stop()
	deadline := time.Now().Add(2 * time.Second)
	for watcher.Status().Coverage == "starting" {
		if time.Now().After(deadline) {
			t.Fatalf("watcher did not report coverage: %+v", watcher.Status())
		}
		time.Sleep(10 * time.Millisecond)
	}
	status := watcher.Status()
	if status.Coverage != "limited" || status.SkippedDirs == 0 {
		t.Fatalf("expected limited coverage, got %+v", status)
	}
}

func TestFlushSkipsRenameSourceOrder(t *testing.T) {
	var batches [][]Event
	watcher := &Watcher{
		notify: func(method string, params any) {
			if method != "watch.events" {
				return
			}
			payload := params.(map[string]any)
			batches = append(batches, payload["events"].([]Event))
		},
		pending: map[string]Event{
			"new": {Path: "new", Kind: "created"},
		},
		order: []string{"new", "old"},
	}
	watcher.flush()
	if len(batches) != 1 || len(batches[0]) != 1 {
		t.Fatalf("batches = %#v", batches)
	}
	if batches[0][0].Path != "new" || batches[0][0].Kind != "created" {
		t.Fatalf("event = %#v", batches[0][0])
	}
}

func TestIgnoredDescriptorClearsPathAndCount(t *testing.T) {
	watcher := &Watcher{
		wdPath: map[int]string{7: "sub"},
		pathWD: map[string]int{"sub": 7},
		status: Status{WatchedDirs: 2},
	}
	watcher.handleEvent(7, unix.IN_IGNORED, 0, "")
	if len(watcher.pathWD) != 0 || len(watcher.wdPath) != 0 {
		t.Fatalf("stale maps: paths=%v descriptors=%v", watcher.pathWD, watcher.wdPath)
	}
	if watcher.status.WatchedDirs != 0 {
		t.Fatalf("watched dirs = %d", watcher.status.WatchedDirs)
	}
}

func TestWatcherReportsRename(t *testing.T) {
	root := t.TempDir()
	events := make(chan Event, 32)
	notify := func(method string, params any) {
		if method != "watch.events" {
			return
		}
		for _, event := range params.(map[string]any)["events"].([]Event) {
			events <- event
		}
	}
	watcher, _, err := Start(Config{Root: root, RootRel: "."}, notify)
	if err != nil {
		t.Fatal(err)
	}
	defer watcher.Stop()
	deadline := time.Now().Add(3 * time.Second)
	for watcher.Status().Coverage != "full" {
		if time.Now().After(deadline) {
			t.Fatalf("watcher did not reach full: %+v", watcher.Status())
		}
		time.Sleep(10 * time.Millisecond)
	}
	old := filepath.Join(root, "old.txt")
	if err := os.WriteFile(old, []byte("data"), 0o600); err != nil {
		t.Fatal(err)
	}
	waitFor := func(match func(Event) bool) Event {
		deadline := time.Now().Add(3 * time.Second)
		for {
			select {
			case event := <-events:
				if match(event) {
					return event
				}
			case <-time.After(time.Until(deadline)):
				t.Fatalf("event not received; status=%+v", watcher.Status())
			}
		}
	}
	waitFor(func(event Event) bool {
		return event.Path == "old.txt" && (event.Kind == "created" || event.Kind == "modified")
	})
	if err := os.Rename(old, filepath.Join(root, "new.txt")); err != nil {
		t.Fatal(err)
	}
	event := waitFor(func(event Event) bool { return event.Kind == "renamed" })
	if event.Path != "old.txt" || event.To != "new.txt" {
		t.Fatalf("rename event = %#v", event)
	}
}

func TestWatcherRewatchesRecreatedDirectory(t *testing.T) {
	root := t.TempDir()
	events := make(chan Event, 32)
	notify := func(method string, params any) {
		if method != "watch.events" {
			return
		}
		for _, event := range params.(map[string]any)["events"].([]Event) {
			events <- event
		}
	}
	watcher, _, err := Start(Config{Root: root, RootRel: "."}, notify)
	if err != nil {
		t.Fatal(err)
	}
	defer watcher.Stop()
	waitFor := func(match func(Event) bool) {
		deadline := time.Now().Add(3 * time.Second)
		for {
			select {
			case event := <-events:
				if match(event) {
					return
				}
			case <-time.After(time.Until(deadline)):
				t.Fatalf("event not received; status=%+v", watcher.Status())
			}
		}
	}
	sub := filepath.Join(root, "sub")
	if err := os.Mkdir(sub, 0o700); err != nil {
		t.Fatal(err)
	}
	waitFor(func(event Event) bool { return event.Path == "sub" && event.IsDir })
	if err := os.RemoveAll(sub); err != nil {
		t.Fatal(err)
	}
	waitFor(func(event Event) bool { return event.Path == "sub" && event.Kind == "removed" })
	deadline := time.Now().Add(3 * time.Second)
	for watcher.Status().WatchedDirs != 1 {
		if time.Now().After(deadline) {
			t.Fatalf("stale watch not removed: %+v", watcher.Status())
		}
		time.Sleep(10 * time.Millisecond)
	}
	if err := os.Mkdir(sub, 0o700); err != nil {
		t.Fatal(err)
	}
	waitFor(func(event Event) bool { return event.Path == "sub" && event.IsDir })
	deadline = time.Now().Add(3 * time.Second)
	for watcher.Status().WatchedDirs != 2 {
		if time.Now().After(deadline) {
			t.Fatalf("recreated directory not watched: %+v", watcher.Status())
		}
		time.Sleep(10 * time.Millisecond)
	}
}
