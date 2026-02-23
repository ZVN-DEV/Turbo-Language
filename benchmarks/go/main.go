package main

import (
	"fmt"
	"os"
)

func main() {
	if len(os.Args) < 2 {
		fmt.Fprintf(os.Stderr, "Usage: %s <fib|trees|matrix|strings|concurrent|all>\n", os.Args[0])
		os.Exit(1)
	}

	arg := os.Args[1]

	switch arg {
	case "fib":
		runFib()
	case "trees":
		runTrees()
	case "matrix":
		runMatrix()
	case "strings":
		runStrings()
	case "concurrent":
		runConcurrent()
	case "all":
		runFib()
		runTrees()
		runMatrix()
		runStrings()
		runConcurrent()
	default:
		fmt.Fprintf(os.Stderr, "Unknown benchmark: %s\n", arg)
		fmt.Fprintf(os.Stderr, "Valid options: fib, trees, matrix, strings, concurrent, all\n")
		os.Exit(1)
	}
}
