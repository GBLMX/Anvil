package fetch

import (
	"context"
	"errors"
	"net"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"testing"
)

type fakeResolver struct {
	addresses []net.IPAddr
	err       error
}

func (r fakeResolver) LookupIPAddr(context.Context, string) ([]net.IPAddr, error) {
	return r.addresses, r.err
}

func testClient(t *testing.T, resolver Resolver) *Client {
	t.Helper()
	client, err := New(Config{
		AllowedHosts:  []string{"example.test", "127.0.0.1"},
		MaxBody:       1024,
		AllowHTTP:     true,
		AllowLoopback: true,
		AllowAnyPort:  true,
		Resolver:      resolver,
	})
	if err != nil {
		t.Fatal(err)
	}
	return client
}

func TestFetchPinsResolvedAddressAndPreservesHost(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if request.Host != "example.test" && !strings.HasPrefix(request.Host, "example.test:") {
			t.Errorf("Host = %q", request.Host)
		}
		_, _ = writer.Write([]byte("hello"))
	}))
	defer server.Close()
	parsed, err := url.Parse(server.URL)
	if err != nil {
		t.Fatal(err)
	}
	resolved := []net.IPAddr{{IP: net.ParseIP("127.0.0.1")}}
	client := testClient(t, fakeResolver{addresses: resolved})
	response, err := client.Fetch(context.Background(), "http://example.test:"+parsed.Port()+"/", 0)
	if err != nil {
		t.Fatal(err)
	}
	if response.Status != http.StatusOK || string(response.Body) != "hello" {
		t.Fatalf("response = %+v", response)
	}
}

func TestFetchRejectsBlockedAddress(t *testing.T) {
	client, err := New(Config{
		AllowedHosts: []string{"example.test"},
		Resolver:     fakeResolver{addresses: []net.IPAddr{{IP: net.ParseIP("127.0.0.1")}}},
	})
	if err != nil {
		t.Fatal(err)
	}
	_, err = client.Fetch(context.Background(), "https://example.test/", 0)
	if !errors.Is(err, ErrBlockedAddress) {
		t.Fatalf("error = %v", err)
	}
}

func TestFetchRejectsMixedAnswers(t *testing.T) {
	client := testClient(t, fakeResolver{addresses: []net.IPAddr{
		{IP: net.ParseIP("93.184.216.34")},
		{IP: net.ParseIP("10.0.0.1")},
	}})
	_, err := client.Fetch(context.Background(), "http://example.test/", 0)
	if !errors.Is(err, ErrBlockedAddress) {
		t.Fatalf("error = %v", err)
	}
}

func TestFetchBodyCap(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) {
		_, _ = writer.Write([]byte("hello"))
	}))
	defer server.Close()
	parsed, _ := url.Parse(server.URL)
	client := testClient(t, fakeResolver{addresses: []net.IPAddr{{IP: net.ParseIP("127.0.0.1")}}})
	response, err := client.Fetch(context.Background(), "http://example.test:"+parsed.Port()+"/", 4)
	if err != nil {
		t.Fatal(err)
	}
	if !response.Truncated || string(response.Body) != "hell" {
		t.Fatalf("response = %+v", response)
	}
}

func TestFetchRejectsRedirect(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if request.URL.Path == "/redirect" {
			http.Redirect(writer, request, "/", http.StatusFound)
			return
		}
		_, _ = writer.Write([]byte("hello"))
	}))
	defer server.Close()
	parsed, _ := url.Parse(server.URL)
	client := testClient(t, fakeResolver{addresses: []net.IPAddr{{IP: net.ParseIP("127.0.0.1")}}})
	_, err := client.Fetch(context.Background(), "http://example.test:"+parsed.Port()+"/redirect", 0)
	if !errors.Is(err, ErrRedirect) {
		t.Fatalf("error = %v", err)
	}
}

func TestFetchRejectsHostNotOnAllowlist(t *testing.T) {
	client := testClient(t, fakeResolver{addresses: []net.IPAddr{{IP: net.ParseIP("93.184.216.34")}}})
	_, err := client.Fetch(context.Background(), "http://other.test/", 0)
	if !errors.Is(err, ErrHostNotAllowed) {
		t.Fatalf("error = %v", err)
	}
}

func TestAddressPolicy(t *testing.T) {
	client := testClient(t, fakeResolver{})
	for _, address := range []string{
		"10.0.0.1", "172.16.1.1", "192.168.1.1", "127.0.0.1",
		"100.64.0.1", "169.254.1.1", "0.0.0.0", "224.0.0.1",
		"::1", "fc00::1", "fe80::1", "::", "ff02::1",
	} {
		if client.addressAllowed(net.ParseIP(address)) && address != "127.0.0.1" && address != "::1" {
			t.Fatalf("%s was allowed", address)
		}
	}
	if !client.addressAllowed(net.ParseIP("93.184.216.34")) {
		t.Fatal("public IPv4 was rejected")
	}
}
