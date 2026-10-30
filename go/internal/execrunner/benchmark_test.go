package execrunner

import (
	"context"
	"testing"
)

func BenchmarkRunTrue(b *testing.B) {
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		result, err := Run(context.Background(), ProcessConfig{
			Path: "/usr/bin/true",
			Env:  []string{},
		})
		if err != nil {
			b.Fatal(err)
		}
		if result.ExitCode != 0 {
			b.Fatalf("exit code = %d", result.ExitCode)
		}
	}
}
