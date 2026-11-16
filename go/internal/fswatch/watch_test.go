package fswatch

import (
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
