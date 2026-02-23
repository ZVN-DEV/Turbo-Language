package main

import (
	"fmt"
	"hash/fnv"
	"strings"
	"time"
)

// generateText produces a deterministic ~1MB ASCII text using a simple LCG PRNG.
func generateText(size int) string {
	// Words to pick from — deterministic vocabulary.
	words := []string{
		"the", "quick", "brown", "fox", "jumps", "over", "lazy", "dog",
		"a", "an", "is", "was", "are", "were", "be", "been",
		"have", "has", "had", "do", "does", "did", "will", "would",
		"could", "should", "may", "might", "shall", "can", "need", "dare",
		"ought", "used", "to", "am", "about", "above", "after", "again",
		"all", "also", "and", "another", "any", "back", "because", "before",
		"between", "both", "but", "by", "came", "come", "day", "each",
		"even", "find", "first", "for", "from", "get", "give", "go",
		"great", "hand", "here", "high", "him", "his", "how", "its",
		"just", "know", "large", "last", "left", "life", "like", "line",
		"long", "look", "made", "make", "man", "many", "most", "much",
		"must", "my", "name", "never", "new", "next", "no", "not",
		"now", "number", "of", "old", "on", "one", "only", "or",
		"other", "our", "out", "own", "part", "people", "place", "point",
		"right", "same", "say", "she", "small", "so", "some", "still",
		"such", "take", "tell", "than", "that", "their", "them", "then",
	}

	var b strings.Builder
	b.Grow(size + 100)

	// LCG parameters (Numerical Recipes).
	seed := uint64(42)
	written := 0

	for written < size {
		seed = seed*6364136223846793005 + 1442695040888963407
		idx := int(seed>>33) % len(words)
		word := words[idx]

		if b.Len() > 0 {
			b.WriteByte(' ')
			written++
		}
		b.WriteString(word)
		written += len(word)
	}

	return b.String()
}

func runStrings() {
	const targetSize = 1024 * 1024 // 1 MB

	text := generateText(targetSize)

	start := time.Now()

	// 1. Count words (split by whitespace).
	wordCount := len(strings.Fields(text))

	// 2. Count occurrences of "the" as a word.
	theCount := 0
	for _, w := range strings.Fields(text) {
		if w == "the" {
			theCount++
		}
	}

	// 3. Reverse the string (rune-aware).
	runes := []rune(text)
	for i, j := 0, len(runes)-1; i < j; i, j = i+1, j-1 {
		runes[i], runes[j] = runes[j], runes[i]
	}
	_ = string(runes)

	// 4. Compute FNV-1a hash of the original text.
	h := fnv.New64a()
	h.Write([]byte(text))
	_ = h.Sum64()

	elapsed := time.Since(start)

	// Use word count as the verification result; theCount is computed but
	// the spec says result = word count.
	_ = theCount

	fmt.Printf("{\"language\":\"go\",\"benchmark\":\"strings\",\"time_ms\":%.3f,\"result\":\"%d\"}\n",
		float64(elapsed.Nanoseconds())/1e6, wordCount)
}
