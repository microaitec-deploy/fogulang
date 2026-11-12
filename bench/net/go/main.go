// The Go side of the network benchmarks, and the load generators every
// language is measured with (so the client is the same for all servers).
//
//	netbench http|ws|tcp|udp PORT            a server (prints "ready")
//	netbench load-http URL CONNS SECONDS     requests on keep-alive connections
//	netbench load-tcp  ADDR CONNS SECONDS    64-byte messages, each waits for its echo
//	netbench load-udp  ADDR CONNS SECONDS    the same with datagrams
//
// The loads print {"count": n, "seconds": s, "errors": e, "p50": ms, "p95": ms, "p99": ms}.
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"sort"
	"strconv"
	"sync"
	"time"

	"github.com/gorilla/websocket"
)

func main() {
	if len(os.Args) < 3 {
		fmt.Fprintln(os.Stderr, "usage: netbench http|ws|tcp|udp PORT | load-http|load-tcp|load-udp TARGET CONNS SECONDS")
		os.Exit(2)
	}
	switch os.Args[1] {
	case "http":
		serveHTTP(os.Args[2])
	case "ws":
		serveWS(os.Args[2])
	case "tcp":
		serveTCP(os.Args[2])
	case "udp":
		serveUDP(os.Args[2])
	case "load-http", "load-tcp", "load-udp":
		conns, _ := strconv.Atoi(os.Args[3])
		secs, _ := strconv.ParseFloat(os.Args[4], 64)
		load(os.Args[1], os.Args[2], conns, time.Duration(secs*float64(time.Second)))
	}
}

func serveHTTP(port string) {
	http.HandleFunc("/hello", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]string{"message": "hello", "path": r.URL.Path})
	})
	ln, err := net.Listen("tcp", "127.0.0.1:"+port)
	check(err)
	fmt.Println("ready")
	check(http.Serve(ln, nil))
}

func serveWS(port string) {
	up := websocket.Upgrader{CheckOrigin: func(*http.Request) bool { return true }}
	http.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		c, err := up.Upgrade(w, r, nil)
		if err != nil {
			return
		}
		defer c.Close()
		for {
			t, msg, err := c.ReadMessage()
			if err != nil {
				return
			}
			if c.WriteMessage(t, msg) != nil {
				return
			}
		}
	})
	ln, err := net.Listen("tcp", "127.0.0.1:"+port)
	check(err)
	fmt.Println("ready")
	check(http.Serve(ln, nil))
}

func serveTCP(port string) {
	ln, err := net.Listen("tcp", "127.0.0.1:"+port)
	check(err)
	fmt.Println("ready")
	for {
		c, err := ln.Accept()
		if err != nil {
			return
		}
		go func() {
			defer c.Close()
			buf := make([]byte, 4096)
			for {
				n, err := c.Read(buf)
				if err != nil {
					return
				}
				if _, err := c.Write(buf[:n]); err != nil {
					return
				}
			}
		}()
	}
}

func serveUDP(port string) {
	c, err := net.ListenPacket("udp", "127.0.0.1:"+port)
	check(err)
	fmt.Println("ready")
	buf := make([]byte, 65536)
	for {
		n, from, err := c.ReadFrom(buf)
		if err != nil {
			return
		}
		c.WriteTo(buf[:n], from)
	}
}

func load(kind, target string, conns int, d time.Duration) {
	payload := make([]byte, 64)
	for i := range payload {
		payload[i] = byte('a' + i%26)
	}
	var mu sync.Mutex
	var lat []float64
	count, errors := 0, 0
	var wg sync.WaitGroup
	start := time.Now()
	deadline := start.Add(d)
	for i := 0; i < conns; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			n, e, l := worker(kind, target, payload, deadline)
			mu.Lock()
			count += n
			errors += e
			lat = append(lat, l...)
			mu.Unlock()
		}()
	}
	wg.Wait()
	secs := time.Since(start).Seconds()
	sort.Float64s(lat)
	pct := func(p float64) float64 {
		if len(lat) == 0 {
			return 0
		}
		return lat[int(float64(len(lat)-1)*p)]
	}
	out, _ := json.Marshal(map[string]any{"count": count, "seconds": secs, "errors": errors, "p50": pct(0.50), "p95": pct(0.95), "p99": pct(0.99)})
	fmt.Println(string(out))
}

// One connection: request, wait for the answer, again, until the deadline.
// Every 16th exchange is timed (the latencies kept stay small).
func worker(kind, target string, payload []byte, deadline time.Time) (n, errs int, lat []float64) {
	var exchange func() error
	switch kind {
	case "load-http":
		c, err := net.Dial("tcp", target[len("http://"):indexOf(target[len("http://"):], '/')+len("http://")])
		if err != nil {
			return 0, 1, nil
		}
		defer c.Close()
		path := target[indexOf(target[len("http://"):], '/')+len("http://"):]
		req := []byte("GET " + path + " HTTP/1.1\r\nHost: bench\r\n\r\n")
		r := bufio.NewReader(c)
		exchange = func() error {
			if _, err := c.Write(req); err != nil {
				return err
			}
			res, err := http.ReadResponse(r, nil)
			if err != nil {
				return err
			}
			_, err = io.Copy(io.Discard, res.Body)
			res.Body.Close()
			if err == nil && res.StatusCode != 200 {
				err = fmt.Errorf("status %d", res.StatusCode)
			}
			return err
		}
	case "load-tcp":
		c, err := net.Dial("tcp", target)
		if err != nil {
			return 0, 1, nil
		}
		defer c.Close()
		buf := make([]byte, len(payload))
		exchange = func() error {
			if _, err := c.Write(payload); err != nil {
				return err
			}
			_, err := io.ReadFull(c, buf)
			return err
		}
	default:
		c, err := net.Dial("udp", target)
		if err != nil {
			return 0, 1, nil
		}
		defer c.Close()
		buf := make([]byte, 2048)
		exchange = func() error {
			if _, err := c.Write(payload); err != nil {
				return err
			}
			c.SetReadDeadline(time.Now().Add(500 * time.Millisecond))
			_, err := c.Read(buf)
			return err
		}
	}
	for time.Now().Before(deadline) {
		timed := n%16 == 0
		var t0 time.Time
		if timed {
			t0 = time.Now()
		}
		if err := exchange(); err != nil {
			errs++
			if kind != "load-udp" {
				return
			}
			continue
		}
		if timed {
			lat = append(lat, float64(time.Since(t0).Microseconds())/1000)
		}
		n++
	}
	return
}

func indexOf(s string, b byte) int {
	for i := 0; i < len(s); i++ {
		if s[i] == b {
			return i
		}
	}
	return len(s)
}

func check(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
