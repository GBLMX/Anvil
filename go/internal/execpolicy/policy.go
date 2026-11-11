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
	case "/usr/bin/git", "/bin/git":
		return "git", nil
	default:
		if filepath.Base(clean) == "rust-analyzer" {
			return "rust-analyzer", nil
		}
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
	case "git":
		return validateGit(args)
	case "rust-analyzer":
		if len(args) == 0 || (len(args) == 1 && args[0] == "--stdio") {
			return nil
		}
		return ErrInvalidArgs
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
		"-S": true, "--smart-case": true, "--files": true, "--hidden": true,
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

func validateGit(args []string) error {
	if len(args) == 4 && args[0] == "--no-pager" && args[1] == "diff" &&
		args[2] == "--name-status" && args[3] == "--diff-filter=U" {
		return nil
	}
	if len(args) > 1 && args[1] == "apply" {
		return validateGitApply(args)
	}
	if len(args) > 1 && args[1] == "blame" {
		return validateGitBlame(args)
	}
	if len(args) == 10 && args[1] == "diff" && args[2] == "--cached" {
		normalized := make([]string, 0, len(args)-1)
		normalized = append(normalized, args[:2]...)
		normalized = append(normalized, args[3:]...)
		args = normalized
	}
	if len(args) != 9 || args[0] != "--no-pager" || args[1] != "diff" ||
		args[2] != "--no-ext-diff" || args[3] != "--no-textconv" ||
		args[4] != "--no-color" ||
		(args[5] != "--unified=0" && args[5] != "--unified=3") ||
		args[6] != "HEAD" || args[7] != "--" {
		return fmt.Errorf("%w: unsupported git command", ErrInvalidArgs)
	}
	path := filepath.Clean(args[8])
	if path == "" || path == "." || path == ".." || filepath.IsAbs(path) ||
		strings.HasPrefix(path, ".."+string(filepath.Separator)) {
		return fmt.Errorf("%w: invalid git path", ErrInvalidArgs)
	}
	return nil
}

func validateGitApply(args []string) error {
	reverse := len(args) == 7 && args[4] == "-R"
	if (!reverse && len(args) != 6) || (reverse && len(args) != 7) ||
		args[0] != "--no-pager" || args[1] != "apply" ||
		args[2] != "--cached" || args[3] != "--whitespace=nowarn" ||
		args[len(args)-2] != "--" || args[len(args)-1] != ".anvil-tmp/git-hunk.patch" {
		return fmt.Errorf("%w: unsupported git apply", ErrInvalidArgs)
	}
	return nil
}

func validateGitBlame(args []string) error {
	if len(args) != 5 || args[0] != "--no-pager" || args[1] != "blame" ||
		args[2] != "--date=short" || args[3] != "--" {
		return fmt.Errorf("%w: unsupported git blame", ErrInvalidArgs)
	}
	path := filepath.Clean(args[4])
	if path == "" || path == "." || path == ".." || filepath.IsAbs(path) ||
		strings.HasPrefix(path, ".."+string(filepath.Separator)) {
		return fmt.Errorf("%w: invalid git path", ErrInvalidArgs)
	}
	return nil
}
