// Word-frequency count baseline (Go).
//
// Reads the file given as argv[1] (or $WORDCOUNT_INPUT), tokenizes on ASCII
// whitespace, counts word frequencies in a map, then prints the top-20 words by
// (count desc, word asc) followed by a final "TOTAL <words> <unique>" line.
// Output must match wordcount.tb byte-for-byte.
package main

import (
	"bufio"
	"fmt"
	"os"
	"sort"
	"strings"
)

const topN = 20

type entry struct {
	word  string
	count int64
}

func main() {
	path := ""
	if len(os.Args) > 1 {
		path = os.Args[1]
	} else if v := os.Getenv("WORDCOUNT_INPUT"); v != "" {
		path = v
	} else {
		path = "wordcount_input.txt"
	}

	data, err := os.ReadFile(path)
	if err != nil {
		fmt.Fprintf(os.Stderr, "cannot read %s: %v\n", path, err)
		os.Exit(1)
	}

	counts := make(map[string]int64)
	var total int64
	for _, word := range strings.Fields(string(data)) {
		counts[word]++
		total++
	}

	unique := len(counts)
	list := make([]entry, 0, unique)
	for w, c := range counts {
		list = append(list, entry{w, c})
	}
	// Sort by count descending, then word ascending.
	sort.Slice(list, func(i, j int) bool {
		if list[i].count != list[j].count {
			return list[i].count > list[j].count
		}
		return list[i].word < list[j].word
	})

	w := bufio.NewWriter(os.Stdout)
	defer w.Flush()
	limit := topN
	if len(list) < limit {
		limit = len(list)
	}
	for i := 0; i < limit; i++ {
		fmt.Fprintf(w, "%s %d\n", list[i].word, list[i].count)
	}
	fmt.Fprintf(w, "TOTAL %d %d\n", total, unique)
}
