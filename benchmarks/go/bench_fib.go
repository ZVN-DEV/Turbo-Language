package main

import (
	"fmt"
	"time"
)

// fib computes the nth Fibonacci number using naive recursion (no memoization).
func fib(n int) int {
	if n <= 1 {
		return n
	}
	return fib(n-1) + fib(n-2)
}

func runFib() {
	start := time.Now()
	result := fib(40)
	elapsed := time.Since(start)

	fmt.Printf("{\"language\":\"go\",\"benchmark\":\"fib\",\"time_ms\":%.3f,\"result\":\"%d\"}\n",
		float64(elapsed.Nanoseconds())/1e6, result)
}
