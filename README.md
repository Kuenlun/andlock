# andlock

A Rust program that counts all valid patterns over a set of nodes in n-dimensional space, generalizing the Android unlock pattern into a purely mathematical combinatorics problem.

---

## Concept

The Android lock screen presents a 3×3 grid of points where the user draws a path connecting at least 4 of them. This program takes that idea as a starting point and treats it as a general mathematical problem.

The nodes are a finite set of points with integer coordinates in n-dimensional space. The program computes the total number of valid patterns that can be drawn over them, applying the same structural rules as the Android lock screen.

---

## Rules

To form a valid pattern, the sequence of nodes must strictly adhere to the following rules:

1. **Uniqueness:** A pattern is an ordered sequence of distinct nodes connected pairwise by straight line segments.
2. **Base Cases:** The empty pattern (length 0) and single-node patterns (length 1) are inherently valid.
3. **Visibility Constraint:** A move from node A to node B is legal only if every node lying strictly on the segment AB has already been visited. Formally, for any intermediate node C such that C = A + t·(B − A) with t ∈ (0, 1), C must appear earlier in the pattern. If no such intermediate node exists, the move is always legal.

---

## Computational Complexity

If we were to ignore the visibility constraint (Rule 3), any sequence of distinct nodes would be valid, and the total count of patterns over N nodes would be exactly `floor(e · N!)`.

While Rule 3 filters out invalid intersections and strictly reduces this number, the total count still scales factorially — remaining roughly on the order of `O(N!)`. Because of this combinatorial explosion, computing the exact number of valid patterns becomes incredibly computationally expensive as N grows.
