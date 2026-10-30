package execrunner

import (
	"fmt"
	"syscall"
)

// Exec replaces the current process with tool.
//
// Apply must run first. The tool path is absolute; no PATH lookup or shell
// parsing occurs.
func Exec(tool string, args, env []string) error {
	if tool == "" || tool[0] != '/' {
		return fmt.Errorf("tool path must be absolute")
	}
	return syscall.Exec(tool, args, env)
}
