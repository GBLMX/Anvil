package main

import (
	"flag"
	"fmt"
	"os"
	"strings"

	"github.com/GBLMX/Anvil/go/internal/execrunner"
)

func main() {
	workspace := flag.String("workspace", "", "workspace root")
	tool := flag.String("tool", "", "absolute executable path")
	runtimePaths := flag.String("runtime-paths", "", "comma-separated read-only runtime paths")
	extraEnv := flag.String("env", "", "comma-separated KEY=VALUE environment entries")
	flag.Parse()
	if *workspace == "" || *tool == "" {
		fmt.Fprintln(os.Stderr, "anvil-exec-runner: -workspace and -tool are required")
		os.Exit(2)
	}
	if err := execrunner.CloseExtraFDs(); err != nil {
		fmt.Fprintf(os.Stderr, "anvil-exec-runner: %v\n", err)
		os.Exit(2)
	}
	paths := execrunner.RuntimePaths()
	for _, path := range strings.Split(*runtimePaths, ",") {
		if path = strings.TrimSpace(path); path != "" {
			paths = append(paths, path)
		}
	}
	if err := execrunner.ApplyTool(*workspace, *tool, paths); err != nil {
		fmt.Fprintf(os.Stderr, "anvil-exec-runner: %v\n", err)
		os.Exit(2)
	}
	args := append([]string{*tool}, flag.Args()...)
	env := []string{
		"PATH=/usr/bin:/bin",
		"LANG=C.UTF-8",
	}
	for _, entry := range strings.Split(*extraEnv, ",") {
		if entry = strings.TrimSpace(entry); entry != "" {
			env = append(env, entry)
		}
	}
	if err := execrunner.Exec(*tool, args, env); err != nil {
		fmt.Fprintf(os.Stderr, "anvil-exec-runner: exec: %v\n", err)
		os.Exit(2)
	}
}
