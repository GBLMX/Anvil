package lsp

import (
	"bufio"
	"bytes"
	"io"
	"strings"
	"testing"
)

func TestFrameRoundTrip(t *testing.T) {
	var encoded bytes.Buffer
	body := []byte(`{"jsonrpc":"2.0","id":1,"method":"initialize"}`)
	if err := WriteFrame(&encoded, body); err != nil {
		t.Fatal(err)
	}
	got, err := ReadFrame(bufio.NewReader(&encoded))
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, body) {
		t.Fatalf("body = %q", got)
	}
}

func TestFrameRejectsMissingAndOversizedHeaders(t *testing.T) {
	if _, err := ReadFrame(bufio.NewReader(strings.NewReader("X: 1\r\n\r\n{}"))); err != ErrMissingHeader {
		t.Fatalf("missing header error = %v", err)
	}
	if _, err := ReadFrame(bufio.NewReader(strings.NewReader("Content-Length: 999999999\r\n\r\n"))); err != ErrTooLarge {
		t.Fatalf("too large error = %v", err)
	}
}

func TestFrameEOF(t *testing.T) {
	if _, err := ReadFrame(bufio.NewReader(bytes.NewReader(nil))); err != io.EOF {
		t.Fatalf("EOF error = %v", err)
	}
}
