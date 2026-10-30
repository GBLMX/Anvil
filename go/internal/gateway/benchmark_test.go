package gateway

import (
	"bytes"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"testing"
)

func BenchmarkPermissionCheck(b *testing.B) {
	server := &Server{cfg: Config{Permissions: []string{"sys.exec"}}}
	state := &connectionState{handshaken: true, permissions: []string{"sys.exec"}}
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		if rpcErr := server.permissionError(state, "sys.exec"); rpcErr != nil {
			b.Fatal(rpcErr)
		}
	}
}

func BenchmarkFrameRoundTrip(b *testing.B) {
	body := []byte(`{"jsonrpc":"2.0","id":"1","method":"sys.fetch","params":{}}`)
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		var buffer bytes.Buffer
		if err := writeFrame(&buffer, body); err != nil {
			b.Fatal(err)
		}
		decoded, err := readFrame(&buffer)
		if err != nil {
			b.Fatal(err)
		}
		if !bytes.Equal(decoded, body) {
			b.Fatal("frame mismatch")
		}
	}
}

func BenchmarkHello(b *testing.B) {
	token := []byte("0123456789abcdef0123456789abcdef")
	hash := sha256Sum(token)
	server := &Server{cfg: Config{TokenSHA256: hash, PluginID: "bench", Permissions: []string{"sys.exec"}}}
	params, err := json.Marshal(helloParams{
		Token:      base64.RawURLEncoding.EncodeToString(token),
		APIVersion: APIVersion,
		Protocol:   Protocol,
	})
	if err != nil {
		b.Fatal(err)
	}
	request := request{JSONRPC: "2.0", ID: stringPtr("1"), Method: "session.hello", Params: params}
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		server.tokenMu.Lock()
		server.tokenUsed = false
		server.tokenMu.Unlock()
		state := &connectionState{permissions: []string{"sys.exec"}}
		if _, rpcErr := server.hello(request, state); rpcErr != nil {
			b.Fatal(rpcErr)
		}
	}
}

func stringPtr(value string) *string { return &value }

func sha256Sum(value []byte) [32]byte { return sha256.Sum256(value) }
