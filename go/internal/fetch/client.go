// Package fetch performs SSRF-constrained HTTP fetches for the gateway.
package fetch

import (
	"context"
	"crypto/tls"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"strings"
	"time"
)

// DefaultMaxBody is the default fetch body limit.
const DefaultMaxBody = 5 << 20

// ErrHostNotAllowed is returned when the host is not on the exact allowlist.
var ErrHostNotAllowed = errors.New("fetch host is not allowed")

// ErrBlockedAddress is returned when any resolved address is non-global.
var ErrBlockedAddress = errors.New("fetch address is not globally routable")

// ErrRedirect is returned for any redirect.
var ErrRedirect = errors.New("redirects are disabled")

// Resolver is the DNS subset needed for pinning.
type Resolver interface {
	LookupIPAddr(ctx context.Context, host string) ([]net.IPAddr, error)
}

// Config controls one fetcher policy.
type Config struct {
	AllowedHosts  []string
	MaxBody       int64
	Timeout       time.Duration
	AllowHTTP     bool // tests only; production is HTTPS-only
	AllowLoopback bool // tests only; production rejects loopback
	AllowAnyPort  bool // tests only; production permits 443 (HTTP tests 80)
	TLSConfig     *tls.Config
	Resolver      Resolver
}

// Response is a bounded fetch result.
type Response struct {
	Status      int
	ContentType string
	Body        []byte
	Truncated   bool
}

// Client is an SSRF-constrained fetcher.
type Client struct {
	cfg     Config
	allowed map[string]struct{}
}

// New creates a fetch client.
func New(cfg Config) (*Client, error) {
	if cfg.MaxBody <= 0 {
		cfg.MaxBody = DefaultMaxBody
	}
	if cfg.Timeout <= 0 {
		cfg.Timeout = 10 * time.Second
	}
	if cfg.Resolver == nil {
		cfg.Resolver = net.DefaultResolver
	}
	if len(cfg.AllowedHosts) == 0 {
		return nil, errors.New("fetch allowlist is empty")
	}
	allowed := make(map[string]struct{}, len(cfg.AllowedHosts))
	for _, host := range cfg.AllowedHosts {
		host = strings.ToLower(strings.TrimSpace(host))
		if host == "" {
			return nil, errors.New("empty fetch host")
		}
		allowed[host] = struct{}{}
	}
	return &Client{cfg: cfg, allowed: allowed}, nil
}

// Fetch performs one GET with DNS pinning, IP validation, redirect rejection,
// and a bounded response body.
func (c *Client) Fetch(ctx context.Context, rawURL string, maxBody int64) (Response, error) {
	if maxBody <= 0 || maxBody > c.cfg.MaxBody {
		maxBody = c.cfg.MaxBody
	}
	parsed, err := url.Parse(rawURL)
	if err != nil || parsed.Hostname() == "" || parsed.User != nil {
		return Response{}, errors.New("invalid fetch URL")
	}
	if parsed.Scheme != "https" && !(c.cfg.AllowHTTP && parsed.Scheme == "http") {
		return Response{}, errors.New("fetch scheme is not allowed")
	}
	host := strings.ToLower(parsed.Hostname())
	if _, ok := c.allowed[host]; !ok {
		return Response{}, ErrHostNotAllowed
	}
	port := parsed.Port()
	if port == "" {
		if parsed.Scheme == "https" {
			port = "443"
		} else {
			port = "80"
		}
	}
	if !c.cfg.AllowAnyPort && ((parsed.Scheme == "https" && port != "443") || (parsed.Scheme == "http" && port != "80")) {
		return Response{}, errors.New("fetch port is not allowed")
	}
	addresses, err := c.cfg.Resolver.LookupIPAddr(ctx, host)
	if err != nil || len(addresses) == 0 {
		return Response{}, fmt.Errorf("resolve fetch host: %w", err)
	}
	var pinned net.IP
	for _, address := range addresses {
		if !c.addressAllowed(address.IP) {
			return Response{}, ErrBlockedAddress
		}
		if pinned == nil {
			pinned = address.IP
		}
	}
	dialAddress := net.JoinHostPort(pinned.String(), port)
	transport := &http.Transport{
		Proxy:                 nil,
		DisableKeepAlives:     true,
		ResponseHeaderTimeout: c.cfg.Timeout,
		TLSClientConfig:       c.cfg.TLSConfig,
		DialContext: func(ctx context.Context, _, _ string) (net.Conn, error) {
			var dialer net.Dialer
			return dialer.DialContext(ctx, "tcp", dialAddress)
		},
	}
	client := &http.Client{
		Transport: transport,
		Timeout:   c.cfg.Timeout,
		CheckRedirect: func(_ *http.Request, _ []*http.Request) error {
			return ErrRedirect
		},
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, parsed.String(), nil)
	if err != nil {
		return Response{}, err
	}
	response, err := client.Do(request)
	if err != nil {
		return Response{}, err
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, maxBody+1))
	if err != nil {
		return Response{}, fmt.Errorf("read fetch body: %w", err)
	}
	truncated := int64(len(body)) > maxBody
	if truncated {
		body = body[:maxBody]
	}
	return Response{
		Status:      response.StatusCode,
		ContentType: response.Header.Get("Content-Type"),
		Body:        body,
		Truncated:   truncated,
	}, nil
}

func (c *Client) addressAllowed(ip net.IP) bool {
	if ip == nil {
		return false
	}
	if ip4 := ip.To4(); ip4 != nil {
		if ip.IsLoopback() || ip.IsPrivate() || ip.IsLinkLocalUnicast() || ip.IsLinkLocalMulticast() ||
			ip.IsUnspecified() || ip.IsMulticast() {
			return c.cfg.AllowLoopback && ip.IsLoopback()
		}
		if ip4[0] == 100 && ip4[1] >= 64 && ip4[1] <= 127 {
			return false
		}
		if ip4[0] == 0 || ip4[0] >= 224 {
			return false
		}
		if ip4[0] == 198 && (ip4[1] == 18 || ip4[1] == 19) {
			return false
		}
		if ip4[0] == 192 && ip4[1] == 0 && ip4[2] == 2 {
			return false
		}
		if ip4[0] == 198 && ip4[1] == 51 && ip4[2] == 100 {
			return false
		}
		if ip4[0] == 203 && ip4[1] == 0 && ip4[2] == 113 {
			return false
		}
		return true
	}
	if ip.IsLoopback() || ip.IsPrivate() || ip.IsLinkLocalUnicast() || ip.IsLinkLocalMulticast() ||
		ip.IsUnspecified() || ip.IsMulticast() {
		return c.cfg.AllowLoopback && ip.IsLoopback()
	}
	if len(ip) == net.IPv6len {
		if ip[0]&0xfe == 0xfc { // fc00::/7
			return false
		}
		if ip[0] == 0x20 && ip[1] == 0x01 && ip[2] == 0x0d && ip[3] == 0xb8 { // 2001:db8::/32
			return false
		}
		if ip[0] == 0x20 && ip[1] == 0x01 && ip[2] == 0x00 && ip[3] == 0x00 { // 2001::/32 Teredo
			return false
		}
		if ip[0] == 0x20 && ip[1] == 0x02 { // 2002::/16 6to4
			return false
		}
		if ip[0] == 0x00 && ip[1] == 0x64 && ip[2] == 0xff && ip[3] == 0x9b { // 64:ff9b::/96 NAT64
			return false
		}
	}
	return true
}
