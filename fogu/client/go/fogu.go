// fogu/fogu.go
package fogu

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"io"
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

func opcodeFromByte(v byte) (Opcode, error) {
	switch Opcode(v) {
	case OpPing, OpPong, OpAuth, OpError,
		OpGet, OpSet, OpDel, OpExists,
		OpIncr, OpDecr,
		OpJoin, OpLeave, OpChannelMsg, OpChannelList, OpMembers,
		OpSubscribe, OpPublish,
		OpCall, OpAct, OpRes,
		OpApiKeyCreate, OpApiKeyDelete, OpApiKeyList, OpApiKeyGet:
		return Opcode(v), nil
	}
	return 0, newFoguError(fmt.Sprintf("opcode desconhecido: 0x%02X", v))
}

type Status byte

const (
	StatusOk           Status = 0x00
	StatusNotFound     Status = 0x01
	StatusError        Status = 0x02
	StatusInvalid      Status = 0x03
	StatusUnauthorized Status = 0x04
	StatusForbidden    Status = 0x05
)

func statusFromByte(v byte) (Status, bool) {
	switch Status(v) {
	case StatusOk, StatusNotFound, StatusError, StatusInvalid, StatusUnauthorized, StatusForbidden:
		return Status(v), true
	}
	return StatusError, false
}

const (
	headerSize = 17
	magic0     = 0x46 // 'F'
	magic1     = 0x4F // 'O'
	version    = 0x01
)

// ============================================================
// Error types
// ============================================================

// FoguError é o tipo de erro principal, equivalente ao enum FoguError do Rust.
type FoguError struct {
	Kind       foguErrorKind
	Message    string
	StatusCode *byte
	Wrapped    error
}

type foguErrorKind int

const (
	errKindMessage foguErrorKind = iota
	errKindProtocol
	errKindIO
	errKindJSON
	errKindTimeout
	errKindNotConnected
	errKindNotAuthenticated
	errKindForbidden
	errKindConnectionClosed
	errKindServerError
)

func (e *FoguError) Error() string {
	switch e.Kind {
	case errKindMessage, errKindProtocol:
		return e.Message
	case errKindIO, errKindJSON:
		if e.Wrapped != nil {
			return e.Wrapped.Error()
		}
		return e.Message
	case errKindTimeout:
		return "timeout"
	case errKindNotConnected:
		return "não conectado"
	case errKindNotAuthenticated:
		return "não autenticado"
	case errKindForbidden:
		return "operação exclusiva do root"
	case errKindConnectionClosed:
		return "conexão encerrada"
	case errKindServerError:
		if e.StatusCode != nil {
			return fmt.Sprintf("server error: %s (status=%d)", e.Message, *e.StatusCode)
		}
		return fmt.Sprintf("server error: %s (status=nil)", e.Message)
	}
	return e.Message
}

func (e *FoguError) Unwrap() error { return e.Wrapped }

func newFoguError(msg string) *FoguError {
	return &FoguError{Kind: errKindMessage, Message: msg}
}

func newProtocolError(msg string) *FoguError {
	return &FoguError{Kind: errKindProtocol, Message: msg}
}

func newServerError(msg string, status *byte) *FoguError {
	return &FoguError{Kind: errKindServerError, Message: msg, StatusCode: status}
}

var (
	errTimeout          = &FoguError{Kind: errKindTimeout}
	errNotConnected     = &FoguError{Kind: errKindNotConnected}
	errNotAuthenticated = &FoguError{Kind: errKindNotAuthenticated}
	errForbidden        = &FoguError{Kind: errKindForbidden}
	errConnectionClosed = &FoguError{Kind: errKindConnectionClosed}
)

// ============================================================
// Frame builder
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

// ============================================================
// URL parser  fogu://token@host:port
// ============================================================

var foguURLRe = regexp.MustCompile(`^fogu://(?:[^:@]+:)?([^@]+)@([^:]+):(\d+)$`)

type foguURL struct {
	token string
	host  string
	port  uint16
}

func parseFoguURL(url string) (foguURL, error) {
	m := foguURLRe.FindStringSubmatch(url)
	if m == nil {
		return foguURL{}, newFoguError("URL inválida. Formato esperado: fogu://token@host:port")
	}
	var port uint16
	if _, err := fmt.Sscanf(m[3], "%d", &port); err != nil {
		return foguURL{}, newFoguError("porta inválida")
	}
	return foguURL{token: m[1], host: m[2], port: port}, nil
}

// ============================================================
// Internal types
// ============================================================

// frameResult representa (Status, Value) ou erro.
type frameResult struct {
	Status Status
	Value  any
	Err    error
}

type msgHandler func(Value any)
type rpcHandler func(params any, ctx *RpcCtx)

// pendingTx é o equivalente ao oneshot::Sender do tokio — canal com buffer 1.
type pendingTx struct {
	ch chan frameResult
}

func newPendingTx() *pendingTx {
	return &pendingTx{ch: make(chan frameResult, 1)}
}

func (p *pendingTx) send(r frameResult) {
	select {
	case p.ch <- r:
	default:
		// Já havia valor — descarta (equivalente a `let _ = tx.send(...)`)
	}
}

// ============================================================
// RpcCtx
// ============================================================

type RpcCtx struct {
	Method    string
	RequestID uint64
	writeTx   chan<- []byte
}

func (ctx *RpcCtx) Reply(result any, errMsg string) {
	var body map[string]any
	if errMsg != "" {
		body = map[string]any{"result": nil, "error": errMsg}
	} else {
		body = map[string]any{"result": result, "error": nil}
	}
	buf, _ := json.Marshal(body)
	select {
	case ctx.writeTx <- buildFrame(OpRes, ctx.RequestID, buf, 0):
	default:
	}
}

// ============================================================
// Inner (shared mutable state)
// ============================================================

type Inner struct {
	connected       bool
	authenticated   bool
	isRoot          bool
	pending         map[uint64]*pendingTx
	channelHandlers map[string][]msgHandler
	topicHandlers   map[string][]msgHandler
	actionHandlers  map[string]rpcHandler
	writeTx         chan<- []byte
}

func newInner() *Inner {
	return &Inner{
		pending:         make(map[uint64]*pendingTx),
		channelHandlers: make(map[string][]msgHandler),
		topicHandlers:   make(map[string][]msgHandler),
		actionHandlers:  make(map[string]rpcHandler),
	}
}

// ============================================================
// FoguClient
// ============================================================

type FoguClient struct {
	inner *sync.Mutex
	state *Inner
	idCtr *uint64
}

func NewFoguClient() *FoguClient {
	return &FoguClient{
		inner: &sync.Mutex{},
		state: newInner(),
		idCtr: new(uint64),
	}
}

func (c *FoguClient) nextID() uint64 {
	return atomic.AddUint64(c.idCtr, 1)
}

func (c *FoguClient) lock()   { c.inner.Lock() }
func (c *FoguClient) unlock() { c.inner.Unlock() }

// clone devolve um "handle" que partilha o mesmo estado (equivalente a #[derive(Clone)]).
func (c *FoguClient) clone() *FoguClient {
	return &FoguClient{
		inner: c.inner,
		state: c.state,
		idCtr: c.idCtr,
	}
}

// ── connect ───────────────────────────────────────────────

func (c *FoguClient) Connect(url string) error {
	parsed, err := parseFoguURL(url)
	if err != nil {
		return err
	}
	addr := fmt.Sprintf("%s:%d", parsed.host, parsed.port)

	conn, err := net.DialTimeout("tcp", addr, 30*time.Second)
	if err != nil {
		if ne, ok := err.(net.Error); ok && ne.Timeout() {
			return errTimeout
		}
		return &FoguError{Kind: errKindIO, Wrapped: err}
	}

	// Canal de escrita (equivalente ao write_tx do tokio)
	writeCh := make(chan []byte, 256)

	c.lock()
	c.state.connected = true
	c.state.writeTx = writeCh
	c.unlock()

	// Writer goroutine
	go func() {
		for frame := range writeCh {
			if _, err := conn.Write(frame); err != nil {
				return
			}
		}
	}()

	// Reader goroutine
	go c.readLoop(conn, writeCh)

	// Envia Auth
	authID := c.nextID()
	authPayload, _ := json.Marshal(map[string]string{"token": parsed.token})

	c.lock()
	ptx := newPendingTx()
	c.state.pending[authID] = ptx
	c.unlock()

	if err := c.sendFrame(OpAuth, authID, authPayload, 0); err != nil {
		return err
	}

	select {
	case r := <-ptx.ch:
		if r.Err != nil {
			c.lock()
			c.state.connected = false
			c.unlock()
			conn.Close()
			return r.Err
		}
		// Extrai role de /result/role
		isRoot := false
		if m, ok := r.Value.(map[string]any); ok {
			if res, ok := m["result"].(map[string]any); ok {
				if role, ok := res["role"].(string); ok && role == "root" {
					isRoot = true
				}
			}
		}
		c.lock()
		c.state.authenticated = true
		c.state.isRoot = isRoot
		c.unlock()
		return nil

	case <-time.After(30 * time.Second):
		c.lock()
		delete(c.state.pending, authID)
		c.state.connected = false
		c.unlock()
		conn.Close()
		return errTimeout
	}
}

// ── readLoop ──────────────────────────────────────────────

func (c *FoguClient) readLoop(conn net.Conn, writeCh chan []byte) {
	buf := make([]byte, 0, 8192)
	tmp := make([]byte, 4096)

	for {
		n, err := conn.Read(tmp)
		if err != nil {
			c.lock()
			c.state.connected = false
			c.state.authenticated = false
			c.state.isRoot = false
			for id, tx := range c.state.pending {
				tx.send(frameResult{Err: errConnectionClosed})
				delete(c.state.pending, id)
			}
			c.unlock()
			return
		}
		buf = append(buf, tmp[:n]...)
		buf = c.processBuffer(buf, writeCh)
	}
}

// ── processBuffer ─────────────────────────────────────────

func (c *FoguClient) processBuffer(buf []byte, writeCh chan []byte) []byte {
	for {
		if len(buf) < headerSize {
			return buf
		}
		if buf[0] != magic0 || buf[1] != magic1 {
			fmt.Println("[FOGU] magic inválido — buffer descartado")
			return nil
		}
		payloadLen := int(binary.BigEndian.Uint32(buf[13:17]))
		totalLen := headerSize + payloadLen
		if len(buf) < totalLen {
			return buf
		}

		opcodeByte := buf[3]
		requestID := binary.BigEndian.Uint64(buf[5:13])
		payload := make([]byte, payloadLen)
		copy(payload, buf[17:totalLen])
		buf = buf[totalLen:]

		opcode, err := opcodeFromByte(opcodeByte)
		if err != nil {
			fmt.Printf("[FOGU] opcode desconhecido: 0x%02X\n", opcodeByte)
			continue
		}

		go c.dispatchFrame(opcode, requestID, payload, writeCh)
	}
}

// ── dispatchFrame ─────────────────────────────────────────

func (c *FoguClient) dispatchFrame(opcode Opcode, requestID uint64, payload []byte, writeCh chan []byte) {
	// Ping → Pong
	if opcode == OpPing {
		select {
		case writeCh <- buildFrame(OpPong, requestID, nil, 0):
		default:
		}
		return
	}

	// ChannelMsg push
	if opcode == OpChannelMsg {
		c.lock()
		_, hasPending := c.state.pending[requestID]
		c.unlock()
		isAck := hasPending && len(payload) > 0 && payload[0] <= 0x05
		if !isAck {
			var envelope map[string]any
			if err := json.Unmarshal(payload, &envelope); err == nil {
				chName, _ := envelope["channel"].(string)
				c.lock()
				handlers := append([]msgHandler(nil), c.state.channelHandlers[chName]...)
				c.unlock()
				for _, h := range handlers {
					h(envelope)
				}
			}
			return
		}
	}

	// Publish push
	if opcode == OpPublish {
		c.lock()
		_, hasPending := c.state.pending[requestID]
		c.unlock()
		if !hasPending {
			var data any
			if err := json.Unmarshal(payload, &data); err != nil {
				data = string(payload)
			}
			c.lock()
			var all []msgHandler
			for _, hs := range c.state.topicHandlers {
				all = append(all, hs...)
			}
			c.unlock()
			for _, h := range all {
				h(data)
			}
			return
		}
	}

	// Call push (RPC server side)
	if opcode == OpCall {
		c.lock()
		_, hasPending := c.state.pending[requestID]
		c.unlock()
		if !hasPending {
			var callData map[string]any
			if err := json.Unmarshal(payload, &callData); err != nil {
				return
			}
			method, _ := callData["method"].(string)
			params := callData["params"]

			c.lock()
			handler, ok := c.state.actionHandlers[method]
			c.unlock()

			if !ok || handler == nil {
				body, _ := json.Marshal(map[string]any{
					"result": nil,
					"error":  fmt.Sprintf("handler '%s' not found", method),
				})
				select {
				case writeCh <- buildFrame(OpRes, requestID, body, 0):
				default:
				}
				return
			}

			ctx := &RpcCtx{
				Method:    method,
				RequestID: requestID,
				writeTx:   writeCh,
			}
			handler(params, ctx)
			return
		}
	}

	// Res (async RPC reply)
	if opcode == OpRes {
		c.lock()
		tx, ok := c.state.pending[requestID]
		if ok {
			delete(c.state.pending, requestID)
		}
		c.unlock()
		if !ok {
			return
		}
		var body map[string]any
		if err := json.Unmarshal(payload, &body); err != nil {
			tx.send(frameResult{Err: newProtocolError("payload inválido em OpRes")})
			return
		}
		if errStr, ok := body["error"].(string); ok && errStr != "" {
			tx.send(frameResult{Err: newServerError(errStr, nil)})
			return
		}
		result, _ := body["result"]
		if result == nil {
			// fallback: usa o body inteiro
			result = body
		}
		tx.send(frameResult{Status: StatusOk, Value: result})
		return
	}

	// Error frame
	if opcode == OpError {
		c.lock()
		tx, ok := c.state.pending[requestID]
		if ok {
			delete(c.state.pending, requestID)
		}
		c.unlock()
		if !ok {
			return
		}

		var statusByte *byte
		if len(payload) > 0 {
			b := payload[0]
			statusByte = &b
		}
		msgBytes := payload
		if len(payload) > 1 {
			msgBytes = payload[1:]
		}
		msgRaw := string(msgBytes)

		errMsg := msgRaw
		var parsed map[string]any
		if err := json.Unmarshal([]byte(msgRaw), &parsed); err == nil {
			if s, ok := parsed["error"].(string); ok {
				errMsg = s
			}
		}
		if errMsg == "" {
			errMsg = "erro desconhecido"
		}
		tx.send(frameResult{Err: newServerError(errMsg, statusByte)})
		return
	}

	// Normal reply
	c.lock()
	tx, ok := c.state.pending[requestID]
	if ok {
		delete(c.state.pending, requestID)
	}
	c.unlock()
	if !ok {
		return
	}

	if len(payload) == 0 {
		tx.send(frameResult{Err: newProtocolError("payload vazio")})
		return
	}

	statusByte := payload[0]
	body := payload[1:]
	status, valid := statusFromByte(statusByte)
	if !valid {
		status = StatusError
	}

	switch status {
	case StatusOk, StatusNotFound:
		var result any
		if len(body) == 0 {
			result = nil
		} else if err := json.Unmarshal(body, &result); err != nil {
			result = string(body)
		}
		tx.send(frameResult{Status: status, Value: result})
	default:
		errStr := string(body)
		errMsg := errStr
		var parsed map[string]any
		if err := json.Unmarshal(body, &parsed); err == nil {
			if s, ok := parsed["error"].(string); ok {
				errMsg = s
			}
		}
		sb := statusByte
		tx.send(frameResult{Err: newServerError(errMsg, &sb)})
	}
}

// ── Guards ────────────────────────────────────────────────

func (c *FoguClient) requireAuth() error {
	c.lock()
	defer c.unlock()
	if !c.state.connected {
		return errNotConnected
	}
	if !c.state.authenticated {
		return errNotAuthenticated
	}
	return nil
}

func (c *FoguClient) requireRoot() error {
	if err := c.requireAuth(); err != nil {
		return err
	}
	c.lock()
	defer c.unlock()
	if !c.state.isRoot {
		return errForbidden
	}
	return nil
}

func (c *FoguClient) sendFrame(opcode Opcode, id uint64, payload []byte, flags byte) error {
	c.lock()
	ch := c.state.writeTx
	connected := c.state.connected
	c.unlock()
	if !connected || ch == nil {
		return errNotConnected
	}
	frame := buildFrame(opcode, id, payload, flags)
	select {
	case ch <- frame:
		return nil
	default:
		return errNotConnected
	}
}

func (c *FoguClient) makeRequest(opcode Opcode, payload any, timeoutMs int64) (Status, any, error) {
	if err := c.requireAuth(); err != nil {
		return 0, nil, err
	}
	id := c.nextID()

	var buf []byte
	if payload != nil {
		var err error
		buf, err = json.Marshal(payload)
		if err != nil {
			return 0, nil, &FoguError{Kind: errKindJSON, Wrapped: err}
		}
	}

	ptx := newPendingTx()
	c.lock()
	c.state.pending[id] = ptx
	c.unlock()

	if err := c.sendFrame(opcode, id, buf, 0); err != nil {
		c.lock()
		delete(c.state.pending, id)
		c.unlock()
		return 0, nil, err
	}

	select {
	case r := <-ptx.ch:
		if r.Err != nil {
			return 0, nil, r.Err
		}
		return r.Status, r.Value, nil

	case <-time.After(time.Duration(timeoutMs) * time.Millisecond):
		c.lock()
		delete(c.state.pending, id)
		c.unlock()
		return 0, nil, errTimeout
	}
}

// ── Public state getters ──────────────────────────────────

func (c *FoguClient) IsConnected() bool {
	c.lock()
	defer c.unlock()
	return c.state.connected
}

func (c *FoguClient) IsAuthenticated() bool {
	c.lock()
	defer c.unlock()
	return c.state.authenticated
}

func (c *FoguClient) IsRoot() bool {
	c.lock()
	defer c.unlock()
	return c.state.isRoot
}

func (c *FoguClient) Disconnect() {
	c.lock()
	defer c.unlock()
	c.state.connected = false
	c.state.authenticated = false
	c.state.isRoot = false
	if c.state.writeTx != nil {
		close(c.state.writeTx)
		c.state.writeTx = nil
	}
}

// ── Ping ──────────────────────────────────────────────────

func (c *FoguClient) Ping() error {
	if err := c.requireAuth(); err != nil {
		return err
	}
	id := c.nextID()
	ptx := newPendingTx()

	c.lock()
	c.state.pending[id] = ptx
	c.unlock()

	if err := c.sendFrame(OpPing, id, nil, 0); err != nil {
		c.lock()
		delete(c.state.pending, id)
		c.unlock()
		return err
	}

	select {
	case r := <-ptx.ch:
		return r.Err
	case <-time.After(5 * time.Second):
		c.lock()
		delete(c.state.pending, id)
		c.unlock()
		return errTimeout
	}
}

// ── Sub-clients ───────────────────────────────────────────

func (c *FoguClient) KV() *KvClient {
	return &KvClient{c: c.clone()}
}

func (c *FoguClient) ApiKeys() *ApiKeyClient {
	return &ApiKeyClient{c: c.clone()}
}

func (c *FoguClient) Channel(name string) *ChannelClient {
	return &ChannelClient{c: c.clone(), name: name}
}

// ============================================================
// KvClient
// ============================================================

type KvClient struct{ c *FoguClient }

func (kv *KvClient) Get(key string) (any, error) {
	_, d, err := kv.c.makeRequest(OpGet, map[string]any{"key": key}, 5000)
	if err != nil {
		return nil, err
	}
	if m, ok := d.(map[string]any); ok {
		return m["value"], nil
	}
	return nil, nil
}

func (kv *KvClient) Set(key string, value any, ttl *int64) (bool, error) {
	p := map[string]any{"key": key, "value": value}
	if ttl != nil {
		p["ttl"] = *ttl
	}
	_, _, err := kv.c.makeRequest(OpSet, p, 5000)
	if err != nil {
		return false, err
	}
	return true, nil
}

func (kv *KvClient) Del(key string) (bool, error) {
	_, d, err := kv.c.makeRequest(OpDel, map[string]any{"key": key}, 5000)
	if err != nil {
		return false, err
	}
	if m, ok := d.(map[string]any); ok {
		if n, ok := m["count"].(float64); ok {
			return n > 0, nil
		}
	}
	return false, nil
}

func (kv *KvClient) Exists(key string) (bool, error) {
	_, d, err := kv.c.makeRequest(OpExists, map[string]any{"key": key}, 5000)
	if err != nil {
		return false, err
	}
	if m, ok := d.(map[string]any); ok {
		if b, ok := m["exists"].(bool); ok {
			return b, nil
		}
	}
	return false, nil
}

func (kv *KvClient) Incr(key string, delta int64) (int64, error) {
	_, d, err := kv.c.makeRequest(OpIncr, map[string]any{"key": key, "delta": delta}, 5000)
	if err != nil {
		return 0, err
	}
	if m, ok := d.(map[string]any); ok {
		if v, ok := m["value"].(float64); ok {
			return int64(v), nil
		}
	}
	return 0, nil
}

func (kv *KvClient) Decr(key string, delta int64) (int64, error) {
	_, d, err := kv.c.makeRequest(OpDecr, map[string]any{"key": key, "delta": delta}, 5000)
	if err != nil {
		return 0, err
	}
	if m, ok := d.(map[string]any); ok {
		if v, ok := m["value"].(float64); ok {
			return int64(v), nil
		}
	}
	return 0, nil
}

// ============================================================
// ApiKeyClient
// ============================================================

type Permissions struct {
	Channels []string `json:"channels"`
	KV       []string `json:"kv"`
	Topics   []string `json:"topics"`
	RPC      []string `json:"rpc"`
}

type ApiKeyClient struct{ c *FoguClient }

func (ak *ApiKeyClient) Create(name string, perms Permissions) (any, error) {
	if err := ak.c.requireRoot(); err != nil {
		return nil, err
	}
	p := map[string]any{
		"name":     name,
		"channels": perms.Channels,
		"kv":       perms.KV,
		"topics":   perms.Topics,
		"rpc":      perms.RPC,
	}
	_, d, err := ak.c.makeRequest(OpApiKeyCreate, p, 5000)
	if err != nil {
		return nil, err
	}
	if m, ok := d.(map[string]any); ok {
		if k, ok := m["api_key"]; ok {
			return k, nil
		}
	}
	return d, nil
}

func (ak *ApiKeyClient) Delete(id string) (any, error) {
	if err := ak.c.requireRoot(); err != nil {
		return nil, err
	}
	_, d, err := ak.c.makeRequest(OpApiKeyDelete, map[string]any{"id": id}, 5000)
	if err != nil {
		return nil, err
	}
	if m, ok := d.(map[string]any); ok {
		if r, ok := m["result"]; ok {
			return r, nil
		}
	}
	return d, nil
}

func (ak *ApiKeyClient) List() ([]any, error) {
	if err := ak.c.requireRoot(); err != nil {
		return nil, err
	}
	_, d, err := ak.c.makeRequest(OpApiKeyList, nil, 5000)
	if err != nil {
		return nil, err
	}
	if m, ok := d.(map[string]any); ok {
		if arr, ok := m["api_keys"].([]any); ok {
			return arr, nil
		}
	}
	return []any{}, nil
}

func (ak *ApiKeyClient) Get(id string) (any, error) {
	if err := ak.c.requireRoot(); err != nil {
		return nil, err
	}
	_, d, err := ak.c.makeRequest(OpApiKeyGet, map[string]any{"id": id}, 5000)
	if err != nil {
		return nil, err
	}
	if m, ok := d.(map[string]any); ok {
		if k, ok := m["api_key"]; ok {
			return k, nil
		}
	}
	return d, nil
}

// ============================================================
// ChannelClient
// ============================================================

type ChannelClient struct {
	c    *FoguClient
	name string
}

func (ch *ChannelClient) ns(sub string) string {
	if sub != "" {
		return ch.name + "/" + sub
	}
	return ch.name
}

func (ch *ChannelClient) Join(alias string) (any, error) {
	p := map[string]any{"channel": ch.name}
	if alias != "" {
		p["alias"] = alias
	}
	_, d, err := ch.c.makeRequest(OpJoin, p, 5000)
	if err != nil {
		return nil, err
	}
	if m, ok := d.(map[string]any); ok {
		if r, ok := m["result"]; ok {
			return r, nil
		}
	}
	return d, nil
}

func (ch *ChannelClient) Leave() (any, error) {
	_, d, err := ch.c.makeRequest(OpLeave, map[string]any{"channel": ch.name}, 5000)
	if err != nil {
		return nil, err
	}
	if m, ok := d.(map[string]any); ok {
		if r, ok := m["result"]; ok {
			return r, nil
		}
	}
	return d, nil
}

func (ch *ChannelClient) Send(data any) (bool, error) {
	_, _, err := ch.c.makeRequest(OpChannelMsg, map[string]any{
		"channel": ch.name, "data": data,
	}, 5000)
	if err != nil {
		return false, err
	}
	return true, nil
}

// OnMessage regista um handler. Devolve uma função de unsubscribe.
func (ch *ChannelClient) OnMessage(fn func(Value any)) func() {
	name := ch.name

	ch.c.lock()
	ch.c.state.channelHandlers[name] = append(ch.c.state.channelHandlers[name], fn)
	ch.c.unlock()

	return func() {
		ch.c.lock()
		defer ch.c.unlock()
		list := ch.c.state.channelHandlers[name]
		for i := range list {
			if fmt.Sprintf("%p", list[i]) == fmt.Sprintf("%p", fn) {
				ch.c.state.channelHandlers[name] = append(list[:i], list[i+1:]...)
				break
			}
		}
		if len(ch.c.state.channelHandlers[name]) == 0 {
			delete(ch.c.state.channelHandlers, name)
		}
	}
}

func (ch *ChannelClient) Members() ([]any, error) {
	_, d, err := ch.c.makeRequest(OpMembers, map[string]any{"channel": ch.name}, 5000)
	if err != nil {
		return nil, err
	}
	if m, ok := d.(map[string]any); ok {
		if arr, ok := m["members"].([]any); ok {
			return arr, nil
		}
	}
	return []any{}, nil
}

func (ch *ChannelClient) List() ([]any, error) {
	_, d, err := ch.c.makeRequest(OpChannelList, nil, 5000)
	if err != nil {
		return nil, err
	}
	if m, ok := d.(map[string]any); ok {
		if arr, ok := m["channels"].([]any); ok {
			return arr, nil
		}
	}
	return []any{}, nil
}

// ── Pub/Sub ───────────────────────────────────────────────

func (ch *ChannelClient) Subscribe(topic string, fn func(Value any)) (func(), error) {
	key := ch.ns(topic)

	ch.c.lock()
	ch.c.state.topicHandlers[key] = append(ch.c.state.topicHandlers[key], fn)
	ch.c.unlock()

	if _, _, err := ch.c.makeRequest(OpSubscribe, map[string]any{"topic": key}, 5000); err != nil {
		ch.c.lock()
		list := ch.c.state.topicHandlers[key]
		for i := range list {
			if fmt.Sprintf("%p", list[i]) == fmt.Sprintf("%p", fn) {
				ch.c.state.topicHandlers[key] = append(list[:i], list[i+1:]...)
				break
			}
		}
		if len(ch.c.state.topicHandlers[key]) == 0 {
			delete(ch.c.state.topicHandlers, key)
		}
		ch.c.unlock()
		return nil, err
	}

	return func() {
		ch.c.lock()
		defer ch.c.unlock()
		list := ch.c.state.topicHandlers[key]
		for i := range list {
			if fmt.Sprintf("%p", list[i]) == fmt.Sprintf("%p", fn) {
				ch.c.state.topicHandlers[key] = append(list[:i], list[i+1:]...)
				break
			}
		}
		if len(ch.c.state.topicHandlers[key]) == 0 {
			delete(ch.c.state.topicHandlers, key)
		}
	}, nil
}

func (ch *ChannelClient) Publish(topic string, data any) (bool, error) {
	_, _, err := ch.c.makeRequest(OpPublish, map[string]any{
		"topic": ch.ns(topic), "data": data,
	}, 5000)
	if err != nil {
		return false, err
	}
	return true, nil
}

// ── RPC ───────────────────────────────────────────────────

func (ch *ChannelClient) Act(method string, fn func(params any, ctx *RpcCtx)) (bool, error) {
	key := ch.ns(method)

	ch.c.lock()
	ch.c.state.actionHandlers[key] = fn
	ch.c.unlock()

	if _, _, err := ch.c.makeRequest(OpAct, map[string]any{"method": key}, 5000); err != nil {
		ch.c.lock()
		delete(ch.c.state.actionHandlers, key)
		ch.c.unlock()
		return false, err
	}
	return true, nil
}

func (ch *ChannelClient) Call(method string, params any, timeoutMs int64) (any, error) {
	if err := ch.c.requireAuth(); err != nil {
		return nil, err
	}
	key := ch.ns(method)
	id := ch.c.nextID()

	pay, _ := json.Marshal(map[string]any{"method": key, "params": params})

	ptx := newPendingTx()
	ch.c.lock()
	ch.c.state.pending[id] = ptx
	ch.c.unlock()

	if err := ch.c.sendFrame(OpCall, id, pay, 0); err != nil {
		ch.c.lock()
		delete(ch.c.state.pending, id)
		ch.c.unlock()
		return nil, err
	}

	select {
	case r := <-ptx.ch:
		if r.Err != nil {
			return nil, r.Err
		}
		return r.Value, nil
	case <-time.After(time.Duration(timeoutMs) * time.Millisecond):
		ch.c.lock()
		delete(ch.c.state.pending, id)
		ch.c.unlock()
		return nil, errTimeout
	}
}

// ============================================================
// Helpers internos
// ============================================================

// decodeJSON é o equivalente a serde_json::from_slice com fallback.
func decodeJSON(data []byte) (any, error) {
	var v any
	if err := json.Unmarshal(data, &v); err != nil {
		return nil, err
	}
	return v, nil
}

// readAll é usado se precisares ler o stream todo (não usado aqui, mas útil).
func readAll(r io.Reader) ([]byte, error) {
	var buf bytes.Buffer
	_, err := io.Copy(&buf, r)
	return buf.Bytes(), err
}
