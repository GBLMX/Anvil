package main

import (
	"context"
	"encoding/hex"
	"flag"
	"fmt"
	"log"
	"os"
	"os/signal"
	"path/filepath"
	"strings"
	"syscall"
	"time"

	"github.com/GBLMX/Anvil/go/internal/fetch"
	"github.com/GBLMX/Anvil/go/internal/gateway"
)

func main() {
	socket := flag.String("socket", "", "gateway Unix socket path")
	workspace := flag.String("workspace", "", "workspace root")
	allowRead := flag.Bool("allow-read", false, "grant sys.read_file")
	allowWrite := flag.Bool("allow-write", false, "grant sys.write_file")
	pluginID := flag.String("plugin-id", "plugin", "registered plugin id")
	tokenHash := flag.String("token-sha256", "", "hex SHA-256 of the 32-byte plugin token")
	lspTokenHash := flag.String("lsp-token-sha256", "", "hex SHA-256 of the LSP worker token")
	permissions := flag.String("permissions", "", "comma-separated granted permissions")
	execRunner := flag.String("exec-runner", "", "absolute anvil-exec-runner path")
	allowExec := flag.Bool("allow-exec", false, "grant sys.exec")
	noSandboxLSP := flag.Bool("no-sandbox-lsp", false, "run LSP tools without Landlock/seccomp")
	execTimeout := flag.Duration("exec-timeout", 5*time.Second, "default sys.exec timeout")
	maxOutput := flag.Int("max-output", 1<<20, "combined exec output limit")
	cgroupRoot := flag.String("cgroup-root", "", "delegated cgroup v2 root for per-session exec limits")
	allowFetch := flag.Bool("allow-fetch", false, "grant sys.fetch")
	fetchAllow := flag.String("fetch-allow", "", "comma-separated exact fetch hosts")
	fetchMax := flag.Int64("fetch-max", fetch.DefaultMaxBody, "maximum fetch body bytes")
	fetchTimeout := flag.Duration("fetch-timeout", 10*time.Second, "fetch timeout")
	flag.Parse()

	if *socket == "" || *workspace == "" || *tokenHash == "" {
		fmt.Fprintln(os.Stderr, "anvil-gateway: -socket, -workspace, and -token-sha256 are required")
		os.Exit(2)
	}
	hash, err := hex.DecodeString(*tokenHash)
	if err != nil || len(hash) != 32 {
		fmt.Fprintln(os.Stderr, "anvil-gateway: -token-sha256 must be 64 hex characters")
		os.Exit(2)
	}
	var lspHash [32]byte
	if *lspTokenHash != "" {
		decoded, err := hex.DecodeString(*lspTokenHash)
		if err != nil || len(decoded) != 32 {
			fmt.Fprintln(os.Stderr, "anvil-gateway: -lsp-token-sha256 must be 64 hex characters")
			os.Exit(2)
		}
		copy(lspHash[:], decoded)
	}
	workspacePath, err := filepath.Abs(*workspace)
	if err != nil {
		fmt.Fprintf(os.Stderr, "anvil-gateway: workspace: %v\n", err)
		os.Exit(2)
	}
	var runnerPath string
	if *execRunner != "" {
		runnerPath, err = filepath.Abs(*execRunner)
		if err != nil {
			fmt.Fprintf(os.Stderr, "anvil-gateway: exec runner: %v\n", err)
			os.Exit(2)
		}
	}
	if *allowExec && runnerPath == "" {
		fmt.Fprintln(os.Stderr, "anvil-gateway: -allow-exec requires -exec-runner")
		os.Exit(2)
	}
	granted := splitPermissions(*permissions)
	if *allowRead && !contains(granted, "fs.read:**") {
		granted = append(granted, "fs.read:**")
	}
	if *allowWrite && !contains(granted, "fs.write:**") {
		granted = append(granted, "fs.write:**")
	}
	if *allowExec && !contains(granted, "sys.exec") {
		granted = append(granted, "sys.exec")
	}
	var fetcher *fetch.Client
	if *allowFetch {
		hosts := splitPermissions(*fetchAllow)
		if len(hosts) == 0 {
			fmt.Fprintln(os.Stderr, "anvil-gateway: -allow-fetch requires -fetch-allow")
			os.Exit(2)
		}
		fetcher, err = fetch.New(fetch.Config{
			AllowedHosts: hosts,
			MaxBody:      *fetchMax,
			Timeout:      *fetchTimeout,
			Audit: func(name string) {
				log.Printf("sys.fetch sensitive header name=%s", name)
			},
		})
		if err != nil {
			fmt.Fprintf(os.Stderr, "anvil-gateway: fetch: %v\n", err)
			os.Exit(2)
		}
		if !contains(granted, "sys.fetch") {
			granted = append(granted, "sys.fetch")
		}
	}
	server, err := gateway.Listen(gateway.Config{
		SocketPath:     *socket,
		Workspace:      workspacePath,
		PluginID:       *pluginID,
		TokenSHA256:    [32]byte(hash),
		LSPTokenSHA256: lspHash,
		Permissions:    granted,
		ExecRunner:     runnerPath,
		NoSandboxLSP:   *noSandboxLSP,
		ExecTimeout:    *execTimeout,
		MaxOutput:      *maxOutput,
		Fetcher:        fetcher,
		CgroupRoot:     *cgroupRoot,
	})
	if err != nil {
		fmt.Fprintf(os.Stderr, "anvil-gateway: %v\n", err)
		os.Exit(2)
	}
	defer server.Close()
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	fmt.Fprintf(os.Stderr, "anvil-gateway: listening on %s\n", server.Addr())
	if err := server.Serve(ctx); err != nil {
		fmt.Fprintf(os.Stderr, "anvil-gateway: %v\n", err)
		os.Exit(2)
	}
}

func splitPermissions(value string) []string {
	if value == "" {
		return nil
	}
	var permissions []string
	for _, permission := range strings.Split(value, ",") {
		if permission = strings.TrimSpace(permission); permission != "" {
			permissions = append(permissions, permission)
		}
	}
	return permissions
}

func contains(values []string, wanted string) bool {
	for _, value := range values {
		if value == wanted {
			return true
		}
	}
	return false
}
