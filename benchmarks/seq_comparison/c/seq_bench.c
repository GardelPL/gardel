#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <omp.h>
#include <time.h>

typedef uint64_t U64;

typedef struct Seq {
    enum { SEQ_Empty, SEQ_Leaf, SEQ_Concat } tag;
    U64 len;
    union {
        struct { U64 value; } Leaf;
        struct { struct Seq* left; struct Seq* right; } Concat;
    };
} Seq;

static inline Seq* seq_empty(void) {
    Seq* s = (Seq*)malloc(sizeof(Seq));
    s->tag = SEQ_Empty;
    s->len = 0;
    return s;
}

static inline Seq* seq_singleton(U64 x) {
    Seq* s = (Seq*)malloc(sizeof(Seq));
    s->tag = SEQ_Leaf;
    s->len = 1;
    s->Leaf.value = x;
    return s;
}

static inline Seq* seq_concat(Seq* l, Seq* r) {
    if (l->tag == SEQ_Empty) { free(l); return r; }
    if (r->tag == SEQ_Empty) { free(r); return l; }
    Seq* s = (Seq*)malloc(sizeof(Seq));
    s->tag = SEQ_Concat;
    s->len = l->len + r->len;
    s->Concat.left = l;
    s->Concat.right = r;
    return s;
}

Seq* seq_from_range(U64 lo, U64 hi) {
    if (lo > hi) return seq_empty();
    if (lo == hi) return seq_singleton(lo);
    U64 mid = lo + (hi - lo) / 2;
    Seq* left = NULL;
    Seq* right = NULL;
    #pragma omp task shared(left) if(hi - lo > 512)
    left = seq_from_range(lo, mid);
    #pragma omp task shared(right) if(hi - lo > 512)
    right = seq_from_range(mid + 1, hi);
    #pragma omp taskwait
    return seq_concat(left, right);
}

U64 seq_parallel_sum(const Seq* s) {
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

void seq_free(Seq* s) {
    if (!s) return;
    if (s->tag == SEQ_Concat) {
        seq_free(s->Concat.left);
        seq_free(s->Concat.right);
    }
    free(s);
}

static double now_ms(void) {
    return omp_get_wtime() * 1000.0;
}

int main(int argc, char** argv) {
    U64 n = argc > 1 ? (U64)strtoull(argv[1], NULL, 10) : 1000000;
    if (n == 0) n = 1;

    Seq* xs = NULL;
    U64 total = 0;

    double t0 = now_ms();
    #pragma omp parallel
    #pragma omp single
    {
        xs = seq_from_range(1, n);
    }
    double t_build = now_ms() - t0;

    double t1 = now_ms();
    #pragma omp parallel
    #pragma omp single
    {
        total = seq_parallel_sum(xs);
    }
    double t_sum = now_ms() - t1;

    U64 expected = n * (n + 1) / 2;
    if (total != expected) {
        fprintf(stderr, "Checksum mismatch: got %llu, expected %llu\n",
                (unsigned long long)total, (unsigned long long)expected);
        return 1;
    }

    printf("RESULT build_ms=%.2f sum_ms=%.2f total_ms=%.2f sum=%llu\n",
           t_build, t_sum, t_build + t_sum, (unsigned long long)total);

    // Explicit cleanup
    seq_free(xs);
    return 0;
}

