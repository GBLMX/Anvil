package execrunner

import (
	"math"
	"testing"

	"golang.org/x/sys/unix"
)

func TestSeccompProgramLengthRejectsOverflow(t *testing.T) {
	instructions := make([]unix.SockFilter, math.MaxUint16+1)
	if _, err := seccompProgramLen(instructions); err == nil {
		t.Fatal("expected an oversized seccomp program to be rejected")
	}
}
