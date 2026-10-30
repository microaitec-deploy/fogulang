// fogu/fogu.go
package fogu

import (
	"encoding/binary"
	"encoding/json"
	"fmt"
	"net"
	"regexp"
	"sync"
	"sync/atomic"
	"time"
)

// ============================================================
// Protocol constants
// ============================================================

type Opcode byte

const (
	OpPing         Opcode = 0x01
	OpPong         Opcode = 0x02
	OpAuth         Opcode = 0x50
	OpError        Opcode = 0xFF
	OpGet          Opcode = 0x10
	OpSet          Opcode = 0x11
	OpDel          Opcode = 0x12
	OpExists       Opcode = 0x13
	OpIncr         Opcode = 0x20
	OpDecr         Opcode = 0x21
	OpJoin         Opcode = 0x30
	OpLeave        Opcode = 0x31
	OpChannelMsg   Opcode = 0x32
	OpChannelList  Opcode = 0x33
	OpMembers      Opcode = 0x34
	OpSubscribe    Opcode = 0x40
	OpPublish      Opcode = 0x41
	OpCall         Opcode = 0x60
	OpAct          Opcode = 0x61
	OpRes          Opcode = 0x62
	OpApiKeyCreate Opcode = 0x70
	OpApiKeyDelete Opcode = 0x71
	OpApiKeyList   Opcode = 0x72
	OpApiKeyGet    Opcode = 0x73
)

type Status byte

const (
	StatusOk           Status = 0x00
	StatusNotFound     Status = 0x01
	StatusError        Status = 0x02
	StatusInvalid      Status = 0x03
	StatusUnauthorized Status = 0x04
	StatusForbidden    Status = 0x05
)

const (
	headerSize = 17
	magic0     = 0x46 // 'F'
	magic1     = 0x4F // 'O'
	version    = 0x01
)

// ============================================================
// FoguError
// ============================================================

type FoguError struct {
	Message    string
	StatusCode *Status
}

func (e *FoguError) Error() string { return e.Message }

func newFoguError(msg string, status ...Status) *FoguError {
	e := &FoguError{Message: msg}
	if len(status) > 0 {
		s := status[0]
		e.StatusCode = &s
	}
	return e
}

// ============================================================
// Frame helpers
// ============================================================

func buildFrame(opcode Opcode, requestID uint64, payload []byte, flags byte) []byte {
	buf := make([]byte, headerSize+len(payload))
	buf[0] = magic0
	buf[1] = magic1
	buf[2] = version
	buf[3] = byte(opcode)
	buf[4] = flags
	binary.BigEndian.PutUint64(buf[5:13], requestID)
	binary.BigEndian.PutUint32(buf[13:17], uint32(len(payload)))
	copy(buf[17:], payload)
	return buf
}

func jsonBuf(v any) ([]byte, error) {
	return json.Marshal(v)
}

func tryParseJSON(data []byte) any {
	var result any
	if err := json.Unmarshal(data, &result); err != nil {
		return string(data)
	}
	return result
}

// ============================================================
// URL parser  fogu://token@host:port
// ============================================================

var foguURLRe = regexp.MustCompile(`^fogu://(?:[^:@]+:)?([^@]+)@([^:]+):(\d+)$`)

type foguAddr struct {
	token string
	host  string
	port  string
}

func parseFoguURL(rawURL string) (foguAddr, error) {
	m := foguURLRe.FindStringSubmatch(rawURL)
	if m == nil {
		return foguAddr{}, fmt.Errorf("URL inválida. Formato esperado: fogu://token@host:port")
	}
	return foguAddr{token: m[1], host: m[2], port: m[3]}, nil
}

// ============================================================
// Pending request
// ============================================================

type pendingReq struct {
	ch    chan response
	timer *time.Timer
}

type response struct {
	status Status
	data   any
	err    error
}

// ============================================================
// Handler types
// ============================================================

type ChannelMsgHandler func(envelope map[string]any)
type TopicHandler func(data any)

type RPCContext struct {
	Method    string
	RequestID uint64
	reply     func(result any, errMsg string)
}

func (ctx *RPCContext) Reply(result any, errMsg string) {
	ctx.reply(result, errMsg)
}

type ActionHandler func(params any, ctx RPCContext)

// ============================================================
// Client
// ============================================================

type Client struct {
	mu            sync.Mutex
	conn          net.Conn
	connected     bool
	authenticated bool
	isRoot        bool

	idCounter uint64 // atomic

	recvBuf []byte

	pending            map[uint64]*pendingReq
	channelMsgHandlers map[string][]ChannelMsgHandler
	topicHandlers      map[string][]TopicHandler
	actionHandlers     map[string]ActionHandler
}

func NewClientFogu() *Client {
	return &Client{
		pending:            make(map[uint64]*pendingReq),
		channelMsgHandlers: make(map[string][]ChannelMsgHandler),
		topicHandlers:      make(map[string][]TopicHandler),
		actionHandlers:     make(map[string]ActionHandler),
	}
}

func (c *Client) Connected() bool     { c.mu.Lock(); defer c.mu.Unlock(); return c.connected }
func (c *Client) Authenticated() bool { c.mu.Lock(); defer c.mu.Unlock(); return c.authenticated }
func (c *Client) IsRoot() bool        { c.mu.Lock(); defer c.mu.Unlock(); return c.isRoot }

func (c *Client) nextID() uint64 {
	return atomic.AddUint64(&c.idCounter, 1)
}

// ── Guards ────────────────────────────────────────────────

func (c *Client) requireAuth() error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if !c.connected {
		return newFoguError("não conectado")
	}
	if !c.authenticated {
		return newFoguError("não autenticado")
	}
	return nil
}

func (c *Client) requireRoot() error {
	if err := c.requireAuth(); err != nil {
		return err
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if !c.isRoot {
		return newFoguError("operação exclusiva do root", StatusForbidden)
	}
	return nil
}

// ── Write ─────────────────────────────────────────────────

func (c *Client) writeFrame(opcode Opcode, requestID uint64, payload []byte, flags byte) error {
	c.mu.Lock()
	conn := c.conn
	ok := c.connected
	c.mu.Unlock()
	if !ok || conn == nil {
		return newFoguError("não conectado")
	}
	_, err := conn.Write(buildFrame(opcode, requestID, payload, flags))
	return err
}

// ── makeRequest ───────────────────────────────────────────

func (c *Client) makeRequest_(opcode Opcode, payload any, timeout time.Duration) (response, error) {
	if err := c.requireAuth(); err != nil {
		return response{}, err
	}

	id := c.nextID()
	var buf []byte
	if payload != nil {
		var err error
		buf, err = json.Marshal(payload)
		if err != nil {
			return response{}, err
		}
	}
	if err := c.writeFrame(opcode, id, buf, 0); err != nil {
		return response{}, err
	}

	ch := make(chan response, 1)
	pr := &pendingReq{ch: ch}
	pr.timer = time.AfterFunc(timeout, func() {
		c.mu.Lock()
		if _, ok := c.pending[id]; ok {
			delete(c.pending, id)
			c.mu.Unlock()
			ch <- response{err: newFoguError(fmt.Sprintf("timeout (opcode 0x%02x)", opcode))}
		} else {
			c.mu.Unlock()
		}
	})

	c.mu.Lock()
	c.pending[id] = pr
	c.mu.Unlock()

	r := <-ch
	return r, r.err
}

func (c *Client) makeRequest(opcode Opcode, payload any, timeout time.Duration) (response, error) {
	if err := c.requireAuth(); err != nil {
		return response{}, err
	}

	if timeout <= 0 {
		return response{}, newFoguError("timeout must be greater than zero")
	}

	id := c.nextID()

	// Serializa payload
	var buf []byte

	if payload != nil {
		var err error

		buf, err = json.Marshal(payload)
		if err != nil {
			return response{}, err
		}
	}

	// Canal da resposta
	ch := make(chan response, 1)

	// Regista ANTES de enviar
	c.mu.Lock()
	c.pending[id] = &pendingReq{
		ch: ch,
	}
	c.mu.Unlock()

	// Envia o frame
	if err := c.writeFrame(opcode, id, buf, 0); err != nil {
		c.mu.Lock()
		delete(c.pending, id)
		c.mu.Unlock()

		return response{}, err
	}

	// Timeout
	timer := time.NewTimer(timeout)
	defer timer.Stop()

	// Aguarda resposta ou timeout
	select {
	case r := <-ch:
		return r, r.err

	case <-timer.C:
		c.mu.Lock()

		// Só remove se ainda estiver pendente.
		_, ok := c.pending[id]

		if ok {
			delete(c.pending, id)
		}

		c.mu.Unlock()

		if !ok {
			// Uma resposta acabou de remover o pending.
			// Espera a resposta que já foi enviada para ch.
			r := <-ch
			return r, r.err
		}

		return response{
			err: newFoguError(
				fmt.Sprintf("timeout (opcode 0x%02x)", opcode),
			),
		}, nil
	}
}

// ============================================================
// Read loop
// ============================================================

func (c *Client) readLoop() {
	buf := make([]byte, 4096)
	for {
		n, err := c.conn.Read(buf)
		if err != nil {
			c.mu.Lock()
			c.connected = false
			c.authenticated = false
			c.isRoot = false
			for id, p := range c.pending {
				p.timer.Stop()
				p.ch <- response{err: newFoguError("conexão encerrada")}
				delete(c.pending, id)
			}
			c.mu.Unlock()
			return
		}
		c.recvBuf = append(c.recvBuf, buf[:n]...)
		c.processBuffer()
	}
}

func (c *Client) processBuffer() {
	for len(c.recvBuf) >= headerSize {
		if c.recvBuf[0] != magic0 || c.recvBuf[1] != magic1 {
			fmt.Println("[FOGU] magic inválido — buffer descartado")
			c.recvBuf = nil
			return
		}
		payloadLen := int(binary.BigEndian.Uint32(c.recvBuf[13:17]))
		total := headerSize + payloadLen
		if len(c.recvBuf) < total {
			break
		}

		opcode := Opcode(c.recvBuf[3])
		requestID := binary.BigEndian.Uint64(c.recvBuf[5:13])
		payload := make([]byte, payloadLen)
		copy(payload, c.recvBuf[17:total])
		c.recvBuf = c.recvBuf[total:]

		c.dispatchFrame(opcode, requestID, payload)
	}
}

// ============================================================
// Frame dispatcher
// ============================================================

func (c *Client) dispatchFrame(opcode Opcode, requestID uint64, payload []byte) {
	// Ping → reply Pong
	if opcode == OpPing {
		_ = c.writeFrame(OpPong, requestID, nil, 0)
		return
	}

	// ── ChannelMsg push ──────────────────────────────────
	if opcode == OpChannelMsg {
		c.mu.Lock()
		_, hasPending := c.pending[requestID]
		c.mu.Unlock()
		isAck := hasPending && len(payload) > 0 && payload[0] <= 0x05
		if !isAck {
			var envelope map[string]any
			if err := json.Unmarshal(payload, &envelope); err == nil {
				ch, _ := envelope["channel"].(string)
				c.mu.Lock()
				handlers := append([]ChannelMsgHandler(nil), c.channelMsgHandlers[ch]...)
				c.mu.Unlock()
				for _, fn := range handlers {
					fn(envelope)
				}
			}
			return
		}
	}

	// ── Publish push ────────────────────────────────────
	if opcode == OpPublish {
		c.mu.Lock()
		_, hasPending := c.pending[requestID]
		c.mu.Unlock()
		if !hasPending {
			data := tryParseJSON(payload)
			c.mu.Lock()
			var all []TopicHandler
			for _, handlers := range c.topicHandlers {
				all = append(all, handlers...)
			}
			c.mu.Unlock()
			for _, fn := range all {
				fn(data)
			}
			return
		}
	}

	// ── Call push (RPC handler side) ─────────────────────
	if opcode == OpCall {
		c.mu.Lock()
		_, hasPending := c.pending[requestID]
		c.mu.Unlock()
		if !hasPending {
			var params map[string]interface{}
			_ = json.Unmarshal(payload, &params)
			c.mu.Lock()

			method := params["method"].(string)
			handler := c.actionHandlers[method]
			c.mu.Unlock()
			if c.actionHandlers[method] == nil {
				return
			}

			rid := requestID
			handler(params["params"], RPCContext{
				Method:    method,
				RequestID: rid,
				reply: func(result any, errMsg string) {
					var body []byte
					if errMsg != "" {
						body, _ = json.Marshal(map[string]any{"result": nil, "error": errMsg})
					} else {
						body, _ = json.Marshal(map[string]any{"result": result, "error": nil})
					}
					_ = c.writeFrame(OpRes, rid, body, 0)
				},
			})
			return
		}
	}

	// ── Res (async RPC reply) ──────────────────────────
	if opcode == OpRes {

		c.mu.Lock()
		p, ok := c.pending[requestID]
		if ok {
			delete(c.pending, requestID)
		}
		c.mu.Unlock()
		if !ok {
			return
		}
		p.timer.Stop()
		var body map[string]any
		fmt.Println(body, "body")
		if err := json.Unmarshal(payload, &body); err == nil {
			if errVal, ok := body["error"]; ok && errVal != nil {
				p.ch <- response{err: newFoguError(fmt.Sprintf("%v", errVal))}
				return
			}
			p.ch <- response{data: body["result"]}
		} else {
			p.ch <- response{data: nil}
		}
		return
	}

	// ── Error frame ──────────────────────────────────────
	if opcode == OpError {
		c.mu.Lock()
		p, ok := c.pending[requestID]
		if ok {
			delete(c.pending, requestID)
		}
		c.mu.Unlock()
		if !ok {
			return
		}
		p.timer.Stop()
		var statusCode *Status
		var msg string
		if len(payload) > 0 {
			s := Status(payload[0])
			statusCode = &s
			msg = string(payload[1:])
		} else {
			msg = string(payload)
		}
		var parsed map[string]any
		if err := json.Unmarshal([]byte(msg), &parsed); err == nil {
			if e, ok := parsed["error"].(string); ok {
				msg = e
			}
		}
		if msg == "" {
			msg = "erro desconhecido"
		}
		fe := newFoguError(msg)
		fe.StatusCode = statusCode
		p.ch <- response{err: fe}
		return
	}

	// ── Normal reply ─────────────────────────────────────
	c.mu.Lock()
	p, ok := c.pending[requestID]
	if ok {
		delete(c.pending, requestID)
	}
	c.mu.Unlock()
	if !ok || len(payload) == 0 {
		return
	}
	p.timer.Stop()

	st := Status(payload[0])
	body := payload[1:]

	if st == StatusOk || st == StatusNotFound {
		var result any
		if len(body) > 0 {
			_ = json.Unmarshal(body, &result)
		}
		p.ch <- response{status: st, data: result}
	} else {
		msg := string(body)
		var parsed map[string]any
		if err := json.Unmarshal(body, &parsed); err == nil {
			if e, ok := parsed["error"].(string); ok {
				msg = e
			}
		}
		if msg == "" {
			msg = fmt.Sprintf("status 0x%02x", st)
		}
		p.ch <- response{err: newFoguError(msg, st)}
	}
}

// ============================================================
// Connect / Disconnect / Ping
// ============================================================

func (c *Client) Connect(rawURL string) error {
	addr, err := parseFoguURL(rawURL)
	if err != nil {
		return err
	}

	timeout := 3 * time.Second
	conn, err := net.DialTimeout("tcp", net.JoinHostPort(addr.host, addr.port), timeout)
	if err != nil {
		return err
	}

	c.mu.Lock()
	c.conn = conn
	c.connected = true
	c.mu.Unlock()

	go c.readLoop()

	// Auth
	authID := c.nextID()
	body, _ := json.Marshal(map[string]string{"token": addr.token})
	if err := c.writeFrame(OpAuth, authID, body, 0); err != nil {
		conn.Close()
		return err
	}

	ch := make(chan response, 1)
	pr := &pendingReq{ch: ch}
	pr.timer = time.AfterFunc(timeout, func() {
		c.mu.Lock()
		if _, ok := c.pending[authID]; ok {
			delete(c.pending, authID)
			c.mu.Unlock()
			conn.Close()
			ch <- response{err: newFoguError("timeout aguardando AUTH")}
		} else {
			c.mu.Unlock()
		}
	})

	c.mu.Lock()
	c.pending[authID] = pr
	c.mu.Unlock()

	r := <-ch
	if r.err != nil {
		c.mu.Lock()
		c.connected = false
		c.mu.Unlock()
		conn.Close()
		return r.err
	}

	role := ""
	if m, ok := r.data.(map[string]any); ok {
		if res, ok := m["result"].(map[string]any); ok {
			role, _ = res["role"].(string)
		}
	}

	c.mu.Lock()
	c.authenticated = true
	c.isRoot = role == "root"
	c.mu.Unlock()
	return nil
}

func (c *Client) Disconnect() {
	c.mu.Lock()
	c.connected = false
	c.authenticated = false
	c.isRoot = false
	conn := c.conn
	c.mu.Unlock()
	if conn != nil {
		conn.Close()
	}
}

func (c *Client) Ping() error {
	timeout := 3 * time.Second
	if err := c.requireAuth(); err != nil {
		return err
	}
	id := c.nextID()
	if err := c.writeFrame(OpPing, id, nil, 0); err != nil {
		return err
	}

	ch := make(chan response, 1)
	pr := &pendingReq{ch: ch}
	pr.timer = time.AfterFunc(timeout, func() {
		c.mu.Lock()
		if _, ok := c.pending[id]; ok {
			delete(c.pending, id)
			c.mu.Unlock()
			ch <- response{err: newFoguError("ping timeout")}
		} else {
			c.mu.Unlock()
		}
	})
	c.mu.Lock()
	c.pending[id] = pr
	c.mu.Unlock()

	r := <-ch
	return r.err
}

// ============================================================
// KV
// ============================================================

type KV struct{ c *Client }

func (c *Client) KV() *KV { return &KV{c} }

func (kv *KV) Get(key string) (any, error) {
	timeout := 3 * time.Second
	r, err := kv.c.makeRequest(OpGet, map[string]any{"key": key}, timeout)
	if err != nil {
		return nil, err
	}
	if m, ok := r.data.(map[string]any); ok {
		return m["value"], nil
	}
	return nil, nil
}

func (kv *KV) Set(key string, value any, ttl *int) error {
	timeout := 3 * time.Second
	payload := map[string]any{"key": key, "value": value}
	if ttl != nil {
		payload["ttl"] = *ttl
	}
	_, err := kv.c.makeRequest(OpSet, payload, timeout)
	return err
}

func (kv *KV) Del(key string) (bool, error) {
	timeout := 3 * time.Second
	r, err := kv.c.makeRequest(OpDel, map[string]any{"key": key}, timeout)
	if err != nil {
		return false, err
	}
	if m, ok := r.data.(map[string]any); ok {
		if count, ok := m["count"].(float64); ok {
			return count > 0, nil
		}
	}
	return false, nil
}

func (kv *KV) Exists(key string) (bool, error) {
	timeout := 3 * time.Second
	r, err := kv.c.makeRequest(OpExists, map[string]any{"key": key}, timeout)
	if err != nil {
		return false, err
	}
	if m, ok := r.data.(map[string]any); ok {
		if exists, ok := m["exists"].(bool); ok {
			return exists, nil
		}
	}
	return false, nil
}

func (kv *KV) Incr(key string, delta int) (float64, error) {
	timeout := 3 * time.Second
	r, err := kv.c.makeRequest(OpIncr, map[string]any{"key": key, "delta": delta}, timeout)
	if err != nil {
		return 0, err
	}
	if m, ok := r.data.(map[string]any); ok {
		if v, ok := m["value"].(float64); ok {
			return v, nil
		}
	}
	return 0, nil
}

func (kv *KV) Decr(key string, delta int) (float64, error) {
	timeout := 3 * time.Second
	r, err := kv.c.makeRequest(OpDecr, map[string]any{"key": key, "delta": delta}, timeout)
	if err != nil {
		return 0, err
	}
	if m, ok := r.data.(map[string]any); ok {
		if v, ok := m["value"].(float64); ok {
			return v, nil
		}
	}
	return 0, nil
}

// ============================================================
// API Keys (root only)
// ============================================================

type ApiKeyPerms struct {
	Channels []string `json:"channels"`
	KV       []string `json:"kv"`
	Topics   []string `json:"topics"`
	RPC      []string `json:"rpc"`
}

type ApiKeys struct{ c *Client }

func (c *Client) ApiKeys() *ApiKeys { return &ApiKeys{c} }

func (ak *ApiKeys) Create(name string, perms ApiKeyPerms) (any, error) {
	timeout := 3 * time.Second
	if err := ak.c.requireRoot(); err != nil {
		return nil, err
	}
	payload := map[string]any{
		"name":     name,
		"channels": perms.Channels,
		"kv":       perms.KV,
		"topics":   perms.Topics,
		"rpc":      perms.RPC,
	}
	r, err := ak.c.makeRequest(OpApiKeyCreate, payload, timeout)
	if err != nil {
		return nil, err
	}
	if m, ok := r.data.(map[string]any); ok {
		if k, ok := m["api_key"]; ok {
			return k, nil
		}
	}
	return r.data, nil
}

func (ak *ApiKeys) Delete(id string) (any, error) {
	timeout := 3 * time.Second
	if err := ak.c.requireRoot(); err != nil {
		return nil, err
	}
	r, err := ak.c.makeRequest(OpApiKeyDelete, map[string]any{"id": id}, timeout)
	if err != nil {
		return nil, err
	}
	if m, ok := r.data.(map[string]any); ok {
		if res, ok := m["result"]; ok {
			return res, nil
		}
	}
	return r.data, nil
}

func (ak *ApiKeys) List() ([]any, error) {
	timeout := 3 * time.Second
	if err := ak.c.requireRoot(); err != nil {
		return nil, err
	}
	r, err := ak.c.makeRequest(OpApiKeyList, nil, timeout)
	if err != nil {
		return nil, err
	}
	if m, ok := r.data.(map[string]any); ok {
		if keys, ok := m["api_keys"].([]any); ok {
			return keys, nil
		}
	}
	return []any{}, nil
}

func (ak *ApiKeys) Get(id string) (any, error) {
	timeout := 3 * time.Second
	if err := ak.c.requireRoot(); err != nil {
		return nil, err
	}
	r, err := ak.c.makeRequest(OpApiKeyGet, map[string]any{"id": id}, timeout)
	if err != nil {
		return nil, err
	}
	if m, ok := r.data.(map[string]any); ok {
		if k, ok := m["api_key"]; ok {
			return k, nil
		}
	}
	return r.data, nil
}

// ============================================================
// Channel
// ============================================================

type Channel struct {
	c    *Client
	name string
}

func (c *Client) Channel(name string) *Channel { return &Channel{c: c, name: name} }

func (ch *Channel) ns(sub string) string {
	if sub != "" {
		return ch.name + "/" + sub
	}
	return ch.name
}

func (ch *Channel) Join(alias string) (any, error) {
	timeout := 3 * time.Second
	payload := map[string]any{"channel": ch.name}
	if alias != "" {
		payload["alias"] = alias
	}
	r, err := ch.c.makeRequest(OpJoin, payload, timeout)
	if err != nil {
		return nil, err
	}
	if m, ok := r.data.(map[string]any); ok {
		if res, ok := m["result"]; ok {
			return res, nil
		}
	}
	return r.data, nil
}

func (ch *Channel) Leave() (any, error) {
	timeout := 3 * time.Second
	r, err := ch.c.makeRequest(OpLeave, map[string]any{"channel": ch.name}, timeout)
	if err != nil {
		return nil, err
	}
	if m, ok := r.data.(map[string]any); ok {
		if res, ok := m["result"]; ok {
			return res, nil
		}
	}
	return r.data, nil
}

func (ch *Channel) Send(data any) error {
	timeout := 3 * time.Second
	_, err := ch.c.makeRequest(OpChannelMsg, map[string]any{"channel": ch.name, "data": data}, timeout)
	return err
}

// OnMessage registers a handler; returns an unsubscribe func.
func (ch *Channel) OnMessage(fn ChannelMsgHandler) func() {
	ch.c.mu.Lock()
	ch.c.channelMsgHandlers[ch.name] = append(ch.c.channelMsgHandlers[ch.name], fn)
	ch.c.mu.Unlock()
	return func() {
		ch.c.mu.Lock()
		defer ch.c.mu.Unlock()
		handlers := ch.c.channelMsgHandlers[ch.name]
		for i, f := range handlers {
			if fmt.Sprintf("%p", f) == fmt.Sprintf("%p", fn) {
				ch.c.channelMsgHandlers[ch.name] = append(handlers[:i], handlers[i+1:]...)
				break
			}
		}
		if len(ch.c.channelMsgHandlers[ch.name]) == 0 {
			delete(ch.c.channelMsgHandlers, ch.name)
		}
	}
}

func (ch *Channel) Members(timeout time.Duration) ([]any, error) {
	r, err := ch.c.makeRequest(OpMembers, map[string]any{"channel": ch.name}, timeout)
	if err != nil {
		return nil, err
	}
	if m, ok := r.data.(map[string]any); ok {
		if members, ok := m["members"].([]any); ok {
			return members, nil
		}
	}
	return []any{}, nil
}

func (ch *Channel) List() ([]any, error) {
	timeout := 3 * time.Second
	r, err := ch.c.makeRequest(OpChannelList, nil, timeout)
	if err != nil {
		return nil, err
	}
	if m, ok := r.data.(map[string]any); ok {
		if channels, ok := m["channels"].([]any); ok {
			return channels, nil
		}
	}
	return []any{}, nil
}

// ── Topics ────────────────────────────────────────────────

func (ch *Channel) Subscribe(topic string, fn TopicHandler) (func(), error) {
	timeout := 3 * time.Second
	key := ch.ns(topic)
	ch.c.mu.Lock()
	ch.c.topicHandlers[key] = append(ch.c.topicHandlers[key], fn)
	ch.c.mu.Unlock()

	if _, err := ch.c.makeRequest(OpSubscribe, map[string]any{"topic": key}, timeout); err != nil {
		// remove handler on failure
		ch.c.mu.Lock()
		handlers := ch.c.topicHandlers[key]
		for i := range handlers {
			if fmt.Sprintf("%p", handlers[i]) == fmt.Sprintf("%p", fn) {
				ch.c.topicHandlers[key] = append(handlers[:i], handlers[i+1:]...)
				break
			}
		}
		ch.c.mu.Unlock()
		return nil, err
	}

	return func() {
		ch.c.mu.Lock()
		defer ch.c.mu.Unlock()
		handlers := ch.c.topicHandlers[key]
		for i := range handlers {
			if fmt.Sprintf("%p", handlers[i]) == fmt.Sprintf("%p", fn) {
				ch.c.topicHandlers[key] = append(handlers[:i], handlers[i+1:]...)
				break
			}
		}
		if len(ch.c.topicHandlers[key]) == 0 {
			delete(ch.c.topicHandlers, key)
		}
	}, nil
}

func (ch *Channel) Publish(topic string, data any) error {
	timeout := 3 * time.Second
	_, err := ch.c.makeRequest(OpPublish, map[string]any{"topic": ch.ns(topic), "data": data}, timeout)
	return err
}

// ── RPC ───────────────────────────────────────────────────

func (ch *Channel) Act(method string, fn ActionHandler) error {
	timeout := 3 * time.Second
	key := ch.ns(method)
	fmt.Println(key)
	ch.c.mu.Lock()
	ch.c.actionHandlers[key] = fn
	ch.c.mu.Unlock()
	_, err := ch.c.makeRequest(OpAct, map[string]any{"method": key}, timeout)
	return err
}

func (ch *Channel) Call(method string, params any, timeout time.Duration) (any, error) {
	key := ch.ns(method)
	return ch.c.makeRequest(OpCall, map[string]any{"method": key, "params": params}, timeout)
}
