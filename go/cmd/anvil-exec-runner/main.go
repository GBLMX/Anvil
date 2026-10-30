package main

import (
	"flag"
	"fmt"
	"os"

	"github.com/GBLMX/Anvil/go/internal/execrunner"
)

func main() {
	workspace := flag.String("workspace", "", "workspace root")
	tool := flag.String("tool", "", "absolute executable path")
	flag.Parse()
	if *workspace == "" || *tool == "" {
		fmt.Fprintln(os.Stderr, "anvil-exec-runner: -workspace and -tool are required")
		os.Exit(2)
	}
	if err := execrunner.CloseExtraFDs(); err != nil {
		fmt.Fprintf(os.Stderr, "anvil-exec-runner: %v\n", err)
		os.Exit(2)
	}
	if err := execrunner.Apply(*workspace, execrunner.RuntimePaths()); err != nil {
		fmt.Fprintf(os.Stderr, "anvil-exec-runner: %v\n", err)
		os.Exit(2)
	}
	args := append([]string{*tool}, flag.Args()...)
	env := []string{
		"PATH=/usr/bin:/bin",
		"LANG=C.UTF-8",
	}
	if err := execrunner.Exec(*tool, args, env); err != nil {
		fmt.Fprintf(os.Stderr, "anvil-exec-runner: exec: %v\n", err)
		os.Exit(2)
	}
}
