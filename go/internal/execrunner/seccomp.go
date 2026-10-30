package execrunner

import (
	"fmt"
	"unsafe"

	"golang.org/x/sys/unix"
)

// applySeccomp denies network, ptrace, namespace, mount, and privilege-change
// syscalls. Landlock remains the filesystem boundary; this filter is the
// network/process boundary.
func applySeccomp() error {
	blocked := []uint32{
		unix.SYS_SOCKET, unix.SYS_CONNECT, unix.SYS_BIND, unix.SYS_LISTEN,
		unix.SYS_ACCEPT, unix.SYS_ACCEPT4, unix.SYS_SENDTO, unix.SYS_RECVFROM,
		unix.SYS_SENDMSG, unix.SYS_RECVMSG,
		unix.SYS_PTRACE, unix.SYS_PROCESS_VM_READV, unix.SYS_PROCESS_VM_WRITEV,
		unix.SYS_MOUNT, unix.SYS_UMOUNT2, unix.SYS_BPF, unix.SYS_KEYCTL,
		unix.SYS_SETUID, unix.SYS_SETGID, unix.SYS_SETREUID, unix.SYS_SETREGID,
		unix.SYS_SETRESUID, unix.SYS_SETRESGID, unix.SYS_CAPSET,
	}
	instructions := make([]unix.SockFilter, 0, 1+len(blocked)*2+1)
	instructions = append(instructions, unix.SockFilter{
		Code: unix.BPF_LD | unix.BPF_W | unix.BPF_ABS,
		K:    0,
	})
	for _, syscall := range blocked {
		instructions = append(instructions,
			unix.SockFilter{
				Code: unix.BPF_JMP | unix.BPF_JEQ | unix.BPF_K,
				Jf:   1,
				K:    syscall,
			},
			unix.SockFilter{
				Code: unix.BPF_RET | unix.BPF_K,
				K:    unix.SECCOMP_RET_ERRNO | uint32(unix.EPERM),
			},
		)
	}
	instructions = append(instructions, unix.SockFilter{
		Code: unix.BPF_RET | unix.BPF_K,
		K:    unix.SECCOMP_RET_ALLOW,
	})
	program := unix.SockFprog{
		Len:    uint16(len(instructions)),
		Filter: &instructions[0],
	}
	if err := unix.Prctl(
		unix.PR_SET_SECCOMP,
		unix.SECCOMP_MODE_FILTER,
		uintptr(unsafe.Pointer(&program)),
		0,
		0,
	); err != nil {
		return fmt.Errorf("prctl seccomp: %w", err)
	}
	return nil
}
