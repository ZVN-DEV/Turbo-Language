package main

import (
	"fmt"
	"time"
)

// TreeNode represents a node in a binary tree.
type TreeNode struct {
	Left  *TreeNode
	Right *TreeNode
	Value int
}

// buildTree creates a complete binary tree of the given depth.
// Each node gets a deterministic value based on its position.
func buildTree(depth int, index int) *TreeNode {
	if depth == 0 {
		return &TreeNode{Value: index}
	}
	return &TreeNode{
		Left:  buildTree(depth-1, 2*index),
		Right: buildTree(depth-1, 2*index+1),
		Value: index,
	}
}

// checksum traverses the tree and computes a checksum over all node values.
func checksum(node *TreeNode) int {
	if node == nil {
		return 0
	}
	return node.Value + checksum(node.Left) - checksum(node.Right)
}

func runTrees() {
	const depth = 21

	start := time.Now()

	tree := buildTree(depth, 1)
	cs := checksum(tree)

	// Let GC collect — the tree goes out of scope after this function,
	// but we include the alloc+traverse in the timing.
	elapsed := time.Since(start)

	fmt.Printf("{\"language\":\"go\",\"benchmark\":\"trees\",\"time_ms\":%.3f,\"result\":\"%d\"}\n",
		float64(elapsed.Nanoseconds())/1e6, cs)
}
