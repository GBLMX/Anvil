package execrunner

import (
	"errors"
	"fmt"

	"golang.org/x/sys/unix"
)

const closeRangeCloexec = 1 << 2

// CloseExtraFDs marks every descriptor above stderr close-on-exec. It does not
// close them immediately, because the Go runtime still owns some descriptors
// until exec. The kernel closes the marked descriptors across execve, so the
// tool sees only stdin, stdout, and stderr.
func CloseExtraFDs() error {
	err := unix.CloseRange(3, ^uint(0), closeRangeCloexec)
	if err == nil {
		return nil
	}
	if !errors.Is(err, unix.ENOSYS) && !errors.Is(err, unix.EINVAL) {
		return fmt.Errorf("close_range: %w", err)
	}
	for fd := 3; fd < 4096; fd++ {
		_, _, errno := unix.Syscall(unix.SYS_FCNTL, uintptr(fd), unix.F_SETFD, unix.FD_CLOEXEC)
		if errno != 0 && !errors.Is(errno, unix.EBADF) {
			return fmt.Errorf("fcntl fd %d: %w", fd, errno)
		}
	}
	return nil
}
