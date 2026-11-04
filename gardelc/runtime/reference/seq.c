// HAND-WRITTEN REFERENCE RUNTIME embedded in gardelc for module SEQ (parallel balanced tree SEQ[U64], C11 + OpenMP).
// It is NOT compiled from the Gardel source of this module and is NOT covered by verification.
// Benchmarked by scripts/c3_regression.ps1. Build: gcc -std=gnu11 -O3 -fopenmp seq.c
// Usage: seq [n]  (sums 1..n; exit 1 if the sum is wrong)
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <omp.h>

typedef uint64_t U64;

typedef struct SEQ {
    enum { SEQ_Empty, SEQ_Leaf, SEQ_Concat } tag;
    U64 len;
    union {
        struct { U64 value; } Leaf;
        struct { struct SEQ* left; struct SEQ* right; } Concat;
    };
} SEQ;

static inline SEQ* seq_empty(void) {
    SEQ* s = (SEQ*)malloc(sizeof(SEQ));
    s->tag = SEQ_Empty;
    s->len = 0;
    return s;
}

static inline SEQ* seq_singleton(U64 x) {
    SEQ* s = (SEQ*)malloc(sizeof(SEQ));
    s->tag = SEQ_Leaf;
    s->len = 1;
    s->Leaf.value = x;
    return s;
}

static inline SEQ* seq_concat(SEQ* l, SEQ* r) {
    if (l->tag == SEQ_Empty) return r;
    if (r->tag == SEQ_Empty) return l;
    SEQ* s = (SEQ*)malloc(sizeof(SEQ));
    s->tag = SEQ_Concat;
    s->len = l->len + r->len;
    s->Concat.left = l;
    s->Concat.right = r;
    return s;
}

SEQ* seq_from_range(U64 lo, U64 hi) {
    if (lo > hi) return seq_empty();
    if (lo == hi) return seq_singleton(lo);
    U64 mid = lo + (hi - lo) / 2;
    SEQ* left = NULL;
    SEQ* right = NULL;
    #pragma omp task shared(left) if(hi - lo > 512)
    left = seq_from_range(lo, mid);
    #pragma omp task shared(right) if(hi - lo > 512)
    right = seq_from_range(mid + 1, hi);
    #pragma omp taskwait
    return seq_concat(left, right);
}

U64 seq_parallel_sum(SEQ* s) {
    switch (s->tag) {
        case SEQ_Empty: return 0;
        case SEQ_Leaf:  return s->Leaf.value;
        case SEQ_Concat: {
            U64 sl = 0, sr = 0;
            #pragma omp task shared(sl) if(s->len > 512)
            sl = seq_parallel_sum(s->Concat.left);
            #pragma omp task shared(sr) if(s->len > 512)
            sr = seq_parallel_sum(s->Concat.right);
            #pragma omp taskwait
            return sl + sr;
        }
    }
    return 0;
}

int main(int argc, char** argv) {
    /* Task C.3: the range size is an optional argument, for benchmarking. */
    U64 n = argc > 1 ? (U64)strtoull(argv[1], NULL, 10) : 10000;
    if (n == 0) n = 1;
    SEQ* xs = NULL;
    U64 total = 0;
    double t0 = omp_get_wtime() * 1000.0;
    #pragma omp parallel
    #pragma omp single
    {
        xs = seq_from_range(1, n);
    }
    double t_build = omp_get_wtime() * 1000.0 - t0;
    double t1 = omp_get_wtime() * 1000.0;
    #pragma omp parallel
    #pragma omp single
    {
        total = seq_parallel_sum(xs);
    }
    double t_sum = omp_get_wtime() * 1000.0 - t1;
    printf("RESULT build_ms=%.2f sum_ms=%.2f total_ms=%.2f sum=%llu\n",
           t_build, t_sum, t_build + t_sum, (unsigned long long)total);
    printf("Reference runtime SEQ[U64]: len = %llu, parallel_sum(1..%llu) = %llu\n",
           (unsigned long long)xs->len, (unsigned long long)n, (unsigned long long)total);
    return total == n * (n + 1) / 2 ? 0 : 1;
}
