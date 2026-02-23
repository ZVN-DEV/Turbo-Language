package main

import (
	"fmt"
	"time"
)

const matrixSize = 1000

// newMatrix allocates a 1000x1000 float64 matrix as a flat slice with row slices.
func newMatrix() [][]float64 {
	data := make([]float64, matrixSize*matrixSize)
	m := make([][]float64, matrixSize)
	for i := range m {
		m[i] = data[i*matrixSize : (i+1)*matrixSize]
	}
	return m
}

func runMatrix() {
	// Initialize matrices with sequential values.
	a := newMatrix()
	b := newMatrix()

	counter := 1.0
	for i := 0; i < matrixSize; i++ {
		for j := 0; j < matrixSize; j++ {
			a[i][j] = counter
			b[i][j] = counter
			counter++
		}
	}

	c := newMatrix()

	start := time.Now()

	// Standard O(n^3) matrix multiplication.
	for i := 0; i < matrixSize; i++ {
		for k := 0; k < matrixSize; k++ {
			aik := a[i][k]
			for j := 0; j < matrixSize; j++ {
				c[i][j] += aik * b[k][j]
			}
		}
	}

	elapsed := time.Since(start)

	fmt.Printf("{\"language\":\"go\",\"benchmark\":\"matrix\",\"time_ms\":%.3f,\"result\":\"%.6f\"}\n",
		float64(elapsed.Nanoseconds())/1e6, c[0][0])
}
