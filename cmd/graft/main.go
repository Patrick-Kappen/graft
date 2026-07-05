package main

import (
	"os"

	"github.com/Patrick-Kappen/graft/internal/cli"
)

func main() {
	os.Exit(cli.Main(os.Args[1:]))
}
