package jcode

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"sync"
	"sync/atomic"

	"github.com/1jehuang/jcode-go/protocol"
	"github.com/1jehuang/jcode-go/transport"
)

var (
	ErrClosed             = errors.New("jcode client closed")
	ErrSubscriberOverflow = errors.New("jcode event subscriber fell behind")
)

// Options controls client construction and event buffering.
type Options struct {
	ClientName    string
	MaxFrameSize  int
	EventBuffer   int
	RequestBuffer int
}

func (o Options) withDefaults() Options {
	if o.ClientName == "" {
		o.ClientName = protocol.DefaultClient
	}
	if o.EventBuffer <= 0 {
		o.EventBuffer = 128
	}
	if o.RequestBuffer <= 0 {
		o.RequestBuffer = 1
	}
	return o
}

// Event is a server event with its stable kind and forward-compatible fields.
// Fields contains the event object minus v, reply_to, and ev.
type Event struct {
	Frame  protocol.ServerFrame
	Kind   string
	Fields json.RawMessage
}

// Decode unmarshals an event's fields into a caller-provided value.
func (e Event) Decode(value any) error {
	if len(e.Fields) == 0 {
		return nil
	}
	return json.Unmarshal(e.Fields, value)
}

// Subscription receives asynchronous events. Next blocks until an event,
// cancellation, client shutdown, or subscriber backpressure failure.
type Subscription struct {
	client *Client
	id     uint64
	events <-chan Event
	errors <-chan error
	once   sync.Once
}

func (s *Subscription) Next(ctx context.Context) (Event, error) {
	if ctx == nil {
		ctx = context.Background()
	}
	select {
	case event, ok := <-s.events:
		if !ok {
			return Event{}, s.client.subscriptionError(s.id)
		}
		return event, nil
	case err, ok := <-s.errors:
		if !ok {
			return Event{}, ErrClosed
		}
		return Event{}, err
	case <-ctx.Done():
		return Event{}, ctx.Err()
	}
}

func (s *Subscription) Close() { s.once.Do(func() { s.client.unsubscribe(s.id) }) }

// Client is a concurrent, context-aware harness API client.
type Client struct {
	transport transport.Transport
	encoder   *protocol.Encoder
	decoder   *protocol.Decoder
	options   Options
	writeMu   sync.Mutex
	pendingMu sync.Mutex
	pending   map[uint64]chan protocol.ServerFrame
	subsMu    sync.Mutex
	subs      map[uint64]*subscriber
	nextID    atomic.Uint64
	nextSub   atomic.Uint64
	closed    chan struct{}
	closeOnce sync.Once
	closeErr  error
}

type subscriber struct {
	events chan Event
	errors chan error
	done   chan struct{}
	once   sync.Once
	err    error
}

// NewClient starts reading from t and completes the protocol hello handshake.
// The transport is closed if the handshake fails.
func NewClient(ctx context.Context, t transport.Transport, options Options) (*Client, error) {
	if t == nil {
		return nil, errors.New("nil transport")
	}
	if ctx == nil {
		ctx = context.Background()
	}
	o := options.withDefaults()
	c := &Client{
		transport: t, encoder: protocol.NewEncoder(t), decoder: protocol.NewDecoder(t),
		options: o, pending: make(map[uint64]chan protocol.ServerFrame),
		subs: make(map[uint64]*subscriber), closed: make(chan struct{}),
	}
	if o.MaxFrameSize > 0 {
		c.encoder.MaxSize = o.MaxFrameSize
		c.decoder.MaxSize = o.MaxFrameSize
	}
	go c.readLoop()
	fields := struct {
		MinVersion int    `json:"min_version"`
		MaxVersion int    `json:"max_version"`
		Client     string `json:"client"`
	}{1, 1, o.ClientName}
	req, err := protocol.NewRawRequest("hello", fields)
	if err != nil {
		c.Close()
		return nil, err
	}
	frame, err := c.request(ctx, req)
	if err != nil {
		c.Close()
		return nil, err
	}
	if value, ok := frame.Event.(protocol.Error); ok {
		c.Close()
		return nil, fmt.Errorf("hello failed: %s: %s", value.Code, value.Message)
	}
	if _, ok := frame.Event.(protocol.HelloOK); !ok {
		c.Close()
		return nil, fmt.Errorf("unexpected hello reply: %T", frame.Event)
	}
	return c, nil
}

func (c *Client) request(ctx context.Context, req protocol.RawRequest) (protocol.ServerFrame, error) {
	if ctx == nil {
		ctx = context.Background()
	}
	select {
	case <-c.closed:
		return protocol.ServerFrame{}, ErrClosed
	default:
	}
	id := c.nextID.Add(1)
	reply := make(chan protocol.ServerFrame, 1)
	c.pendingMu.Lock()
	c.pending[id] = reply
	c.pendingMu.Unlock()
	frame := protocol.ClientFrame{V: protocol.APIVersionMajor, ID: id, Request: req}
	c.writeMu.Lock()
	err := c.encoder.Write(frame)
	c.writeMu.Unlock()
	if err != nil {
		c.removePending(id)
		c.Close()
		return protocol.ServerFrame{}, err
	}
	select {
	case result, ok := <-reply:
		if !ok {
			return protocol.ServerFrame{}, ErrClosed
		}
		return result, nil
	case <-ctx.Done():
		c.removePending(id)
		return protocol.ServerFrame{}, ctx.Err()
	case <-c.closed:
		c.removePending(id)
		return protocol.ServerFrame{}, ErrClosed
	}
}

// Request sends a raw request and returns its correlated reply.
func (c *Client) Request(ctx context.Context, req protocol.RawRequest) (protocol.ServerFrame, error) {
	return c.request(ctx, req)
}

// Subscribe receives asynchronous events. A full buffer terminates that
// subscription rather than blocking the reader and starving other callers.
func (c *Client) Subscribe(sessionID string) *Subscription {
	buffer := c.options.EventBuffer
	s := &subscriber{events: make(chan Event, buffer), errors: make(chan error, 1), done: make(chan struct{})}
	id := c.nextSub.Add(1)
	c.subsMu.Lock()
	select {
	case <-c.closed:
		s.err = ErrClosed
		close(s.events)
		close(s.errors)
	default:
		c.subs[id] = s
	}
	c.subsMu.Unlock()
	return &Subscription{client: c, id: id, events: s.events, errors: s.errors}
}

func (c *Client) readLoop() {
	for {
		data, err := c.decoder.ReadFrame()
		if err != nil {
			if !errors.Is(err, io.EOF) {
				c.closeWith(err)
			} else {
				c.closeWith(ErrClosed)
			}
			return
		}
		frame, err := protocol.DecodeServerFrame(data)
		if err != nil {
			c.closeWith(err)
			return
		}
		if frame.ReplyTo != nil {
			c.pendingMu.Lock()
			reply := c.pending[*frame.ReplyTo]
			if reply != nil {
				delete(c.pending, *frame.ReplyTo)
			}
			c.pendingMu.Unlock()
			if reply != nil {
				reply <- frame
			}
			continue
		}
		kind := eventKind(frame.Event)
		fields, _ := protocol.FieldsJSON(frame.Event)
		event := Event{Frame: frame, Kind: kind, Fields: fields}
		c.subsMu.Lock()
		for id, sub := range c.subs {
			select {
			case <-sub.done:
				delete(c.subs, id)
			case sub.events <- event:
			default:
				c.failSubscriberLocked(id, sub, ErrSubscriberOverflow)
			}
		}
		c.subsMu.Unlock()
	}
}

func eventKind(event protocol.Event) string {
	switch value := event.(type) {
	case protocol.HelloOK:
		return "hello_ok"
	case protocol.OK:
		return "ok"
	case protocol.Error:
		return "error"
	case protocol.RawEvent:
		return value.Kind
	case protocol.UnknownEvent:
		return value.Kind
	default:
		return ""
	}
}

func (c *Client) removePending(id uint64) {
	c.pendingMu.Lock()
	delete(c.pending, id)
	c.pendingMu.Unlock()
}
func (c *Client) unsubscribe(id uint64) {
	c.subsMu.Lock()
	if sub := c.subs[id]; sub != nil {
		c.closeSubscriberLocked(id, sub, nil)
	}
	c.subsMu.Unlock()
}
func (c *Client) subscriptionError(id uint64) error {
	c.subsMu.Lock()
	defer c.subsMu.Unlock()
	if sub := c.subs[id]; sub != nil && sub.err != nil {
		return sub.err
	}
	return ErrClosed
}
func (c *Client) failSubscriberLocked(id uint64, sub *subscriber, err error) {
	c.closeSubscriberLocked(id, sub, err)
}
func (c *Client) closeSubscriberLocked(id uint64, sub *subscriber, err error) {
	delete(c.subs, id)
	sub.once.Do(func() {
		sub.err = err
		if err != nil {
			sub.errors <- err
		}
		close(sub.done)
		close(sub.events)
		close(sub.errors)
	})
}

func (c *Client) closeWith(err error) {
	c.closeOnce.Do(func() {
		c.closeErr = err
		close(c.closed)
		_ = c.transport.Close()
		c.pendingMu.Lock()
		pending := c.pending
		c.pending = make(map[uint64]chan protocol.ServerFrame)
		c.pendingMu.Unlock()
		for _, ch := range pending {
			close(ch)
		}
		c.subsMu.Lock()
		for id, sub := range c.subs {
			c.closeSubscriberLocked(id, sub, err)
		}
		c.subsMu.Unlock()
	})
}

// Close is idempotent and wakes all pending requests and subscriptions.
func (c *Client) Close() error { c.closeWith(ErrClosed); return c.closeErr }
