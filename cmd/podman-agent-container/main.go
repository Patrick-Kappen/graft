package main

import (
	"os"

	"github.com/zerodawn1990/podman-agent-container/internal/cli"
)

func main() {
	os.Exit(cli.Main(os.Args[1:]))
}
