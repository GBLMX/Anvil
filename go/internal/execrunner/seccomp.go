package execrunner

import (
	"fmt"
	"unsafe"

	"golang.org/x/sys/unix"
)

const (
	seccompDataNr   = 0
	seccompDataArch = 4
	seccompArg0     = 16
	seccompArg2     = 32
	seccompArg3     = 40

	// clone namespace flags cannot be set. clone3 is returned as ENOSYS so
	// glibc falls back to clone, where the flags word is inspectable.
	namespaceFlags = unix.CLONE_NEWTIME | unix.CLONE_NEWNS | unix.CLONE_NEWCGROUP |
		unix.CLONE_NEWUTS | unix.CLONE_NEWIPC | unix.CLONE_NEWUSER |
		unix.CLONE_NEWPID | unix.CLONE_NEWNET
)

var allowedSyscalls = []uint32{
	unix.SYS_READ,
	unix.SYS_WRITE,
	unix.SYS_CLOSE,
	unix.SYS_FSTAT,
	unix.SYS_NEWFSTATAT,
	unix.SYS_STATX,
	unix.SYS_LSEEK,
	unix.SYS_MMAP,
	unix.SYS_MPROTECT,
	unix.SYS_MUNMAP,
	unix.SYS_MADVISE,
	unix.SYS_MREMAP,
	unix.SYS_BRK,
	unix.SYS_RT_SIGACTION,
	unix.SYS_RT_SIGPROCMASK,
	unix.SYS_RT_SIGRETURN,
	unix.SYS_SIGALTSTACK,
	unix.SYS_READLINK,
	unix.SYS_READLINKAT,
	unix.SYS_GETDENTS64,
	unix.SYS_OPEN,
	unix.SYS_OPENAT,
	unix.SYS_GETCWD,
	unix.SYS_EXECVE,
	unix.SYS_EXIT,
	unix.SYS_EXIT_GROUP,
	unix.SYS_ARCH_PRCTL,
	unix.SYS_SET_TID_ADDRESS,
	unix.SYS_SET_ROBUST_LIST,
	unix.SYS_PRLIMIT64,
	unix.SYS_GETRANDOM,
	unix.SYS_GETRLIMIT,
	unix.SYS_CLOCK_GETTIME,
	unix.SYS_CLOCK_NANOSLEEP,
	unix.SYS_GETPID,
	unix.SYS_GETTID,
	unix.SYS_FUTEX,
	unix.SYS_SCHED_YIELD,
	unix.SYS_SCHED_GETAFFINITY,
	unix.SYS_UNAME,
	unix.SYS_POLL,
	unix.SYS_PPOLL,
	unix.SYS_EPOLL_CREATE1,
	unix.SYS_EPOLL_CTL,
	unix.SYS_EPOLL_WAIT,
	unix.SYS_EPOLL_PWAIT,
	unix.SYS_EVENTFD2,
	unix.SYS_ACCESS,
	unix.SYS_RSEQ,
	unix.SYS_TGKILL,
	unix.SYS_CLONE,
	unix.SYS_INOTIFY_INIT,
	unix.SYS_INOTIFY_INIT1,
	unix.SYS_INOTIFY_ADD_WATCH,
	unix.SYS_INOTIFY_RM_WATCH,
}

type jumpFix struct {
	index int
	jt    string
	jf    string
}

type filterBuilder struct {
	insns  []unix.SockFilter
	labels map[string]int
	fixes  []jumpFix
}

func newFilterBuilder() *filterBuilder {
	return &filterBuilder{labels: make(map[string]int)}
}

func (b *filterBuilder) label(name string) {
	if _, exists := b.labels[name]; exists {
		panic("duplicate seccomp label " + name)
	}
	b.labels[name] = len(b.insns)
}

func (b *filterBuilder) ld(offset uint32) {
	b.insns = append(b.insns, unix.SockFilter{
		Code: unix.BPF_LD | unix.BPF_W | unix.BPF_ABS,
		K:    offset,
	})
}

func (b *filterBuilder) and(value uint32) {
	b.insns = append(b.insns, unix.SockFilter{
		Code: unix.BPF_ALU | unix.BPF_AND | unix.BPF_K,
		K:    value,
	})
}

func (b *filterBuilder) ret(value uint32) {
	b.insns = append(b.insns, unix.SockFilter{
		Code: unix.BPF_RET | unix.BPF_K,
		K:    value,
	})
}

func errnoRet(errno unix.Errno) uint32 {
	return unix.SECCOMP_RET_ERRNO | uint32(errno)
}

func (b *filterBuilder) jeq(value uint32, jt, jf string) {
	b.fixes = append(b.fixes, jumpFix{index: len(b.insns), jt: jt, jf: jf})
	b.insns = append(b.insns, unix.SockFilter{
		Code: unix.BPF_JMP | unix.BPF_JEQ | unix.BPF_K,
		K:    value,
	})
}

func (b *filterBuilder) finish() ([]unix.SockFilter, error) {
	for _, fix := range b.fixes {
		next := fix.index + 1
		insn := &b.insns[fix.index]
		if fix.jt != "" {
			offset, ok := b.labels[fix.jt]
			if !ok {
				return nil, fmt.Errorf("seccomp label %q missing", fix.jt)
			}
			offset -= next
			if offset < 0 || offset > 255 {
				return nil, fmt.Errorf("seccomp jump to %q out of range: %d", fix.jt, offset)
			}
			insn.Jt = uint8(offset)
		}
		if fix.jf != "" {
			offset, ok := b.labels[fix.jf]
			if !ok {
				return nil, fmt.Errorf("seccomp label %q missing", fix.jf)
			}
			offset -= next
			if offset < 0 || offset > 255 {
				return nil, fmt.Errorf("seccomp jump to %q out of range: %d", fix.jf, offset)
			}
			insn.Jf = uint8(offset)
		}
	}
	return b.insns, nil
}

func seccompProgram() ([]unix.SockFilter, error) {
	b := newFilterBuilder()
	b.label("start")
	b.ld(seccompDataArch)
	b.jeq(uint32(unix.AUDIT_ARCH_X86_64), "arch_ok", "deny")
	b.label("arch_ok")
	b.ld(seccompDataNr)

	// clone3 carries a pointer to its flags struct, so the flags cannot be
	// inspected in BPF. ENOSYS makes glibc fall back to clone.
	b.jeq(unix.SYS_CLONE3, "clone3", "after_clone3")
	b.label("clone3")
	b.ret(errnoRet(unix.ENOSYS))
	b.label("after_clone3")

	b.jeq(unix.SYS_CLONE, "clone", "after_clone")
	b.label("clone")
	b.ld(seccompArg0)
	b.and(namespaceFlags)
	b.jeq(0, "clone_no_namespace", "deny")
	b.label("clone_no_namespace")
	b.ld(seccompArg0)
	b.and(unix.CLONE_THREAD)
	b.jeq(unix.CLONE_THREAD, "clone_allow", "deny")
	b.label("clone_allow")
	b.ret(unix.SECCOMP_RET_ALLOW)
	b.label("after_clone")

	// File-backed executable mappings are required by ld.so. Anonymous
	// executable mappings are the JIT/loader-free path, so reject them.
	b.jeq(unix.SYS_MMAP, "mmap", "after_mmap")
	b.label("mmap")
	b.ld(seccompArg2)
	b.and(unix.PROT_EXEC)
	b.jeq(0, "mmap_allow", "mmap_check_flags")
	b.label("mmap_check_flags")
	b.ld(seccompArg3)
	b.and(unix.MAP_ANONYMOUS)
	b.jeq(0, "mmap_allow", "deny")
	b.label("mmap_allow")
	b.ret(unix.SECCOMP_RET_ALLOW)
	b.label("after_mmap")

	b.jeq(unix.SYS_MPROTECT, "mprotect", "after_mprotect")
	b.label("mprotect")
	b.ld(seccompArg2)
	b.and(unix.PROT_EXEC)
	b.jeq(0, "mprotect_allow", "deny")
	b.label("mprotect_allow")
	b.ret(unix.SECCOMP_RET_ALLOW)
	b.label("after_mprotect")

	for _, syscall := range allowedSyscalls {
		b.jeq(syscall, "allow", "")
	}
	b.label("deny")
	b.ret(errnoRet(unix.EPERM))
	b.label("allow")
	b.ret(unix.SECCOMP_RET_ALLOW)
	return b.finish()
}

func applySeccomp() error {
	instructions, err := seccompProgram()
	if err != nil {
		return err
	}
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
