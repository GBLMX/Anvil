// Package lsp owns language-server framing and process lifecycle.
package lsp

import (
	"bufio"
	"errors"
	"fmt"
	"io"
	"strconv"
	"strings"
)

// MaxFrame bounds a single LSP JSON-RPC body.
const MaxFrame = 8 << 20

var (
	ErrMissingHeader = errors.New("lsp: missing Content-Length")
	ErrInvalidHeader = errors.New("lsp: invalid Content-Length")
	ErrTooLarge      = errors.New("lsp: frame too large")
)

// ReadFrame reads one Content-Length framed LSP message.
func ReadFrame(reader *bufio.Reader) ([]byte, error) {
	length := -1
	for {
		line, err := reader.ReadString('\n')
		if err != nil {
			if errors.Is(err, io.EOF) && line == "" {
				return nil, io.EOF
			}
			return nil, err
		}
		line = strings.TrimRight(line, "\r\n")
		if line == "" {
			break
		}
		name, value, ok := strings.Cut(line, ":")
		if !ok {
			return nil, ErrInvalidHeader
		}
		if strings.EqualFold(strings.TrimSpace(name), "Content-Length") {
			parsed, err := strconv.Atoi(strings.TrimSpace(value))
			if err != nil || parsed <= 0 {
				return nil, ErrInvalidHeader
			}
			length = parsed
		}
	}
	if length < 0 {
		return nil, ErrMissingHeader
	}
	if length > MaxFrame {
		return nil, ErrTooLarge
	}
	body := make([]byte, length)
	if _, err := io.ReadFull(reader, body); err != nil {
		return nil, err
	}
	return body, nil
}

// WriteFrame writes one Content-Length framed LSP message.
func WriteFrame(writer io.Writer, body []byte) error {
	if len(body) == 0 {
		return fmt.Errorf("lsp: empty body")
	}
	if len(body) > MaxFrame {
		return ErrTooLarge
	}
	if _, err := fmt.Fprintf(writer, "Content-Length: %d\r\n\r\n", len(body)); err != nil {
		return err
	}
	_, err := writer.Write(body)
	return err
}
