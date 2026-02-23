package main

import (
	"fmt"
	"sync"
	"time"
)

// fibSmall computes fib(n) recursively — used for the concurrent benchmark with n=30.
func fibSmall(n int) int {
	if n <= 1 {
		return n
	}
	return fibSmall(n-1) + fibSmall(n-2)
}

func runConcurrent() {
	const numGoroutines = 1000
	const fibN = 30

	start := time.Now()

	results := make(chan int, numGoroutines)
	var wg sync.WaitGroup
	wg.Add(numGoroutines)

	for i := 0; i < numGoroutines; i++ {
		go func() {
			defer wg.Done()
			results <- fibSmall(fibN)
		}()
	}

	// Close the channel once all goroutines finish.
	go func() {
		wg.Wait()
		close(results)
	}()

	sum := 0
	for v := range results {
		sum += v
	}

	elapsed := time.Since(start)

	fmt.Printf("{\"language\":\"go\",\"benchmark\":\"concurrent\",\"time_ms\":%.3f,\"result\":\"%d\"}\n",
		float64(elapsed.Nanoseconds())/1e6, sum)
}
