package transport

import (
	"bufio"
	"bytes"
	"errors"
	"io"
	"sync"
)

var ErrClosed = errors.New("transport closed")

type Transport interface{ io.ReadWriteCloser }

type Pipe struct {
	reader *io.PipeReader
	writer *io.PipeWriter
	once   sync.Once
}

func NewPipePair() (*Pipe, *Pipe) {
	leftRead, rightWrite := io.Pipe()
	rightRead, leftWrite := io.Pipe()
	return &Pipe{reader: leftRead, writer: leftWrite}, &Pipe{reader: rightRead, writer: rightWrite}
}
func (p *Pipe) Read(b []byte) (int, error)  { return p.reader.Read(b) }
func (p *Pipe) Write(b []byte) (int, error) { return p.writer.Write(b) }
func (p *Pipe) Close() error {
	var err error
	p.once.Do(func() { _ = p.reader.Close(); err = p.writer.Close() })
	return err
}

type FakeServer struct {
	side   *Pipe
	reader *bufio.Reader
	done   chan struct{}
}

func NewFakeServer(side *Pipe) *FakeServer {
	return &FakeServer{side: side, reader: bufio.NewReader(side), done: make(chan struct{})}
}
func (s *FakeServer) Send(raw []byte) error {
	if len(raw) == 0 || raw[len(raw)-1] != '\n' {
		raw = append(bytes.Clone(raw), '\n')
	}
	_, err := s.side.Write(raw)
	return err
}
func (s *FakeServer) Receive() ([]byte, error) { return s.reader.ReadBytes('\n') }
func (s *FakeServer) Close() error {
	select {
	case <-s.done:
	default:
		close(s.done)
	}
	return s.side.Close()
}
