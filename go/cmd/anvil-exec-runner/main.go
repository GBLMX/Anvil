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
	execPaths := flag.String("exec-paths", "", "comma-separated read-only executable paths")
	workspaceWrite := flag.Bool("workspace-write", false, "allow writes inside the workspace")
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
	var executablePaths []string
	for _, path := range strings.Split(*execPaths, ",") {
		if path = strings.TrimSpace(path); path != "" {
			executablePaths = append(executablePaths, path)
		}
	}
	var applyErr error
	if *workspaceWrite {
		applyErr = execrunner.ApplyToolForLSP(*workspace, *tool, paths, executablePaths)
	} else {
		applyErr = execrunner.ApplyToolWithExecute(*workspace, *tool, paths, executablePaths)
	}
	if applyErr != nil {
		fmt.Fprintf(os.Stderr, "anvil-exec-runner: %v\n", applyErr)
		os.Exit(2)
	}
	args := append([]string{*tool}, flag.Args()...)
	env := []string{"LANG=C.UTF-8"}
	for _, entry := range strings.Split(*extraEnv, ",") {
		if entry = strings.TrimSpace(entry); entry != "" {
			env = append(env, entry)
		}
	}
	hasPath := false
	for _, entry := range env {
		if strings.HasPrefix(entry, "PATH=") {
			hasPath = true
			break
		}
	}
	if !hasPath {
		env = append(env, "PATH=/usr/bin:/bin")
	}
	if err := execrunner.Exec(*tool, args, env); err != nil {
		fmt.Fprintf(os.Stderr, "anvil-exec-runner: exec: %v\n", err)
		os.Exit(2)
	}
}
