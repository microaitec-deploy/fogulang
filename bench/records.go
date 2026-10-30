// JSON: stringify and parse 50k records
package main

import (
	"encoding/json"
	"fmt"
	"strconv"
)

type row struct {
	Active bool     `json:"active"`
	ID     int      `json:"id"`
	Name   string   `json:"name"`
	Tags   []string `json:"tags"`
}

func main() {
	rows := []row{}
	for i := 0; i < 50000; i++ {
		rows = append(rows, row{ID: i, Name: "user" + strconv.Itoa(i), Active: i%2 == 0, Tags: []string{"a", "b"}})
	}
	text, _ := json.Marshal(rows)
	var back []map[string]any
	if err := json.Unmarshal(text, &back); err != nil {
		panic(err)
	}
	fmt.Println(len(back), back[49999]["name"], len(text))
}
