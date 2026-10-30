package fetch

import (
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/tls"
	"crypto/x509"
	"errors"
	"math/big"
	"net"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"testing"
	"time"
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
	response, err := client.Fetch(context.Background(), "http://example.test:"+parsed.Port()+"/", 0, nil)
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
	_, err = client.Fetch(context.Background(), "https://example.test/", 0, nil)
	if !errors.Is(err, ErrBlockedAddress) {
		t.Fatalf("error = %v", err)
	}
}

func TestFetchRejectsMixedAnswers(t *testing.T) {
	client := testClient(t, fakeResolver{addresses: []net.IPAddr{
		{IP: net.ParseIP("93.184.216.34")},
		{IP: net.ParseIP("10.0.0.1")},
	}})
	_, err := client.Fetch(context.Background(), "http://example.test/", 0, nil)
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
	response, err := client.Fetch(context.Background(), "http://example.test:"+parsed.Port()+"/", 4, nil)
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
	_, err := client.Fetch(context.Background(), "http://example.test:"+parsed.Port()+"/redirect", 0, nil)
	if !errors.Is(err, ErrRedirect) {
		t.Fatalf("error = %v", err)
	}
}

func TestFetchRejectsHostNotOnAllowlist(t *testing.T) {
	client := testClient(t, fakeResolver{addresses: []net.IPAddr{{IP: net.ParseIP("93.184.216.34")}}})
	_, err := client.Fetch(context.Background(), "http://other.test/", 0, nil)
	if !errors.Is(err, ErrHostNotAllowed) {
		t.Fatalf("error = %v", err)
	}
}

func TestFetchTLSValidatesOriginalHost(t *testing.T) {
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	template := &x509.Certificate{
		SerialNumber: big.NewInt(1),
		NotBefore:    time.Now().Add(-time.Hour),
		NotAfter:     time.Now().Add(time.Hour),
		DNSNames:     []string{"example.test"},
		KeyUsage:     x509.KeyUsageDigitalSignature,
		ExtKeyUsage:  []x509.ExtKeyUsage{x509.ExtKeyUsageServerAuth},
	}
	der, err := x509.CreateCertificate(rand.Reader, template, template, &key.PublicKey, key)
	if err != nil {
		t.Fatal(err)
	}
	certificate, err := x509.ParseCertificate(der)
	if err != nil {
		t.Fatal(err)
	}
	roots := x509.NewCertPool()
	roots.AddCert(certificate)
	server := httptest.NewUnstartedServer(http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) {
		_, _ = writer.Write([]byte("tls"))
	}))
	server.TLS = &tls.Config{Certificates: []tls.Certificate{{Certificate: [][]byte{der}, PrivateKey: key}}}
	server.StartTLS()
	defer server.Close()
	parsed, err := url.Parse(server.URL)
	if err != nil {
		t.Fatal(err)
	}
	client, err := New(Config{
		AllowedHosts:  []string{"example.test"},
		AllowLoopback: true,
		AllowAnyPort:  true,
		TLSConfig:     &tls.Config{RootCAs: roots},
		Resolver:      fakeResolver{addresses: []net.IPAddr{{IP: net.ParseIP("127.0.0.1")}}},
	})
	if err != nil {
		t.Fatal(err)
	}
	response, err := client.Fetch(context.Background(), "https://example.test:"+parsed.Port()+"/", 0, nil)
	if err != nil {
		t.Fatal(err)
	}
	if string(response.Body) != "tls" {
		t.Fatalf("body = %q", response.Body)
	}
}

func TestFetchHeadersWhitelistAndAudit(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if request.Header.Get("Accept") != "application/json" {
			t.Errorf("Accept = %q", request.Header.Get("Accept"))
		}
		if request.Header.Get("Authorization") != "Bearer secret" {
			t.Errorf("Authorization = %q", request.Header.Get("Authorization"))
		}
		_, _ = writer.Write([]byte("ok"))
	}))
	defer server.Close()
	parsed, _ := url.Parse(server.URL)
	var audited []string
	client, err := New(Config{
		AllowedHosts:  []string{"example.test"},
		AllowHTTP:     true,
		AllowLoopback: true,
		AllowAnyPort:  true,
		Resolver:      fakeResolver{addresses: []net.IPAddr{{IP: net.ParseIP("127.0.0.1")}}},
		Audit:         func(name string) { audited = append(audited, name) },
	})
	if err != nil {
		t.Fatal(err)
	}
	_, err = client.Fetch(context.Background(), "http://example.test:"+parsed.Port()+"/", 0, map[string]string{
		"Accept":        "application/json",
		"Authorization": "Bearer secret",
	})
	if err != nil {
		t.Fatal(err)
	}
	if len(audited) != 1 || audited[0] != "Authorization" {
		t.Fatalf("audited = %v", audited)
	}
	if _, err := client.Fetch(context.Background(), "http://example.test:"+parsed.Port()+"/", 0, map[string]string{"Cookie": "x"}); err == nil {
		t.Fatal("Cookie header was accepted")
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
