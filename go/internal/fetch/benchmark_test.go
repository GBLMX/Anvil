package fetch

import (
	"context"
	"net"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"testing"
)

func BenchmarkFetchHTTP(b *testing.B) {
	body := strings.Repeat("x", 4096)
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) {
		_, _ = writer.Write([]byte(body))
	}))
	defer server.Close()
	parsed, err := url.Parse(server.URL)
	if err != nil {
		b.Fatal(err)
	}
	client, err := New(Config{
		AllowedHosts:  []string{"example.test"},
		AllowHTTP:     true,
		AllowLoopback: true,
		AllowAnyPort:  true,
		MaxBody:       1 << 20,
		Resolver:      fakeResolver{addresses: []net.IPAddr{{IP: net.ParseIP("127.0.0.1")}}},
	})
	if err != nil {
		b.Fatal(err)
	}
	url := "http://example.test:" + parsed.Port() + "/"
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		response, err := client.Fetch(context.Background(), url, 0, nil)
		if err != nil {
			b.Fatal(err)
		}
		if len(response.Body) != len(body) {
			b.Fatalf("body length = %d", len(response.Body))
		}
	}
}
