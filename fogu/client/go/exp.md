package main

import (
	"fmt"
	"log"
	"time"

	"seu_modulo/fogu"
)

func main() {
	c := fogu.New()

	if err := c.Connect("fogu://TOKEN@127.0.0.1:9090", 30*time.Second); err != nil {
		log.Fatal(err)
	}
	defer c.Disconnect()

	// KV — 0 significa "use o default (5s)"
	_ = c.KV().Set("hello", "world", 0, 0)
	v, _ := c.KV().Get("hello", 0)
	fmt.Println("hello =", v)

	// Canal
	room := c.Channel("sala1")
	_, _ = room.Join("", 0)

	cancel := room.OnMessage(func(env map[string]any) {
		fmt.Println("mensagem:", env)
	})
	defer cancel()

	_ = room.Send(map[string]any{"text": "olá"}, 0)

	// Tópico
	unsub, _ := room.Subscribe("chat", func(data any, env map[string]any) {
		fmt.Println("chat:", data)
	}, 0)
	defer unsub()

	_ = room.Publish("chat", map[string]any{"msg": "oi"}, 0)

	// RPC
	_ = room.Act("soma", func(params any, reply fogu.RPCReply) {
		reply.Result(42)
	}, 0)

	res, _ := room.Call("soma", map[string]any{}, 3*time.Second)
	fmt.Println("rpc:", res)

	// Ping
	if err := c.Ping(0); err != nil {
		log.Println("ping:", err)
	}
}


















	/*
	ch.Act("apps.list", func(params any, ctx foguClient.) {

		/*input := map[string]interface{}{
			"collection": "posts",
			"where":      map[string]interface{}{},
			"sort":       "-createdAt",
			"limit":      20,
			"page":       1,
		}*/

		input := map[string]interface{}{}

		// Use crases (`) para strings multilinha no Go
		inputStr := `{
		"collection": "apps",
		"where": {},
		"sort": "-createdAt",
		"limit": 20,
		"page": 1,
		"depth": 1
	}`

		// Remova a vírgula no final da linha e trate o erro
		err := json.Unmarshal([]byte(inputStr), &input)
		if err != nil {
			info := map[string]string{
				"err": err.Error(),
			}
			ctx.Reply(info, "")
			return
		}

		fmt.Println(input, err)
		res, err := chCms.Call("cms.find", input, 5*time.Second)

		fmt.Println(res, err)

		if err != nil {
			info := map[string]string{
				"err": err.Error(),
			}

			ctx.Reply(info, "")
			return
		}
		ctx.Reply(res, "")
	})

	*/
