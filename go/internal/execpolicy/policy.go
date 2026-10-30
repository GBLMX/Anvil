// Package execpolicy validates the v1 allowlisted tools before an exec request
// reaches the sandbox runner.
package execpolicy

import (
	"errors"
	"fmt"
	"path/filepath"
	"strings"
)

var (
	// ErrUnknownTool means the executable is not in the v1 allowlist.
	ErrUnknownTool = errors.New("executable is not allowlisted")
	// ErrInvalidArgs means an argument can execute code or load configuration.
	ErrInvalidArgs = errors.New("executable arguments are not allowed")
)

// AllowedTool resolves an executable path to its canonical v1 tool name.
func AllowedTool(path string) (string, error) {
	clean := filepath.Clean(path)
	switch clean {
	case "/usr/bin/rg", "/bin/rg":
		return "rg", nil
	case "/usr/bin/fd", "/bin/fd", "/usr/bin/fdfind":
		return "fd", nil
	default:
		return "", fmt.Errorf("%w: %s", ErrUnknownTool, path)
	}
}

// Validate checks an argv array against the per-tool parser.
func Validate(tool string, args []string) error {
	name, err := AllowedTool(tool)
	if err != nil {
		return err
	}
	switch name {
	case "rg":
		return validateRg(args)
	case "fd":
		return validateFd(args)
	default:
		return ErrUnknownTool
	}
}

func validateRg(args []string) error {
	valueFlags := map[string]bool{
		"--glob": true, "--type": true,
	}
	allowedFlags := map[string]bool{
		"-n": true, "--line-number": true, "--json": true,
		"-i": true, "--ignore-case": true, "-F": true, "--fixed-strings": true,
		"-S": true, "--smart-case": true,
	}
	for index := 0; index < len(args); index++ {
		arg := args[index]
		if arg == "--" {
			return nil
		}
		if strings.HasPrefix(arg, "--pre") || strings.HasPrefix(arg, "--config-path") {
			return fmt.Errorf("%w: %s", ErrInvalidArgs, arg)
		}
		if strings.HasPrefix(arg, "--glob=") || strings.HasPrefix(arg, "--type=") {
			continue
		}
		if strings.HasPrefix(arg, "-") {
			if allowedFlags[arg] {
				continue
			}
			if valueFlags[arg] {
				index++
				if index >= len(args) {
					return fmt.Errorf("%w: %s needs a value", ErrInvalidArgs, arg)
				}
				continue
			}
			return fmt.Errorf("%w: unknown flag %s", ErrInvalidArgs, arg)
		}
	}
	return nil
}

func validateFd(args []string) error {
	valueFlags := map[string]bool{
		"--type": true, "--max-depth": true,
	}
	allowedFlags := map[string]bool{
		"--hidden": true, "--json": true,
	}
	for index := 0; index < len(args); index++ {
		arg := args[index]
		if arg == "--" {
			return nil
		}
		if arg == "--exec" || arg == "--exec-batch" ||
			strings.HasPrefix(arg, "--exec=") || strings.HasPrefix(arg, "--exec-batch=") {
			return fmt.Errorf("%w: %s", ErrInvalidArgs, arg)
		}
		if strings.HasPrefix(arg, "--type=") || strings.HasPrefix(arg, "--max-depth=") {
			continue
		}
		if strings.HasPrefix(arg, "-") {
			if allowedFlags[arg] {
				continue
			}
			if valueFlags[arg] {
				index++
				if index >= len(args) {
					return fmt.Errorf("%w: %s needs a value", ErrInvalidArgs, arg)
				}
				continue
			}
			return fmt.Errorf("%w: unknown flag %s", ErrInvalidArgs, arg)
		}
	}
	return nil
}
