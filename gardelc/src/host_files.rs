//! Files and processes for the reactive host (spec 11.6, Task F.11a): `ReadFile`, `ListDir`,
//! `RunProcess` with `WriteProcess`, `CloseInput` and `StopProcess`, and `RunAttached`.
//!
//! Every blocking call runs on a helper thread, whose answer joins the reader's queue, so a
//! command never blocks the loop (spec 11.6, rule 5). A helper builds no Gardel value: it hands
//! over malloc'd bytes, and the loop makes them values as it takes them (`gardel_answer_taken`),
//! so the allocator is only ever used by the loop.
//!
//! The C below is emitted after `REACTIVE_RUNTIME`, whose declarations it uses. The parts that
//! name instance types (`LIST_STRING`, `RESULT_IO_ERROR_BYTES`, ...) are in `typed_runtime`.

/// What the host does for files and processes, apart from building the events' values.
pub(crate) const HOST_FILES_RUNTIME: &str = r#"/* Files and processes (spec 11.6). */
#define GARDEL_FILE_MAX (256u * 1024u * 1024u) /* the longest file ReadFile reads */
#define GARDEL_PROC_READ 65536

/* A running process. The loop owns the table; helper threads hold references, counted under the
   lock, and the last one out frees it. `running` counts the threads that must end before
   ProcessExited: the two readers and the waiter, or the waiter alone for RunAttached. So every
   output event comes before ProcessExited, and none after it. */
struct gardel_proc {
    uint64_t id, tag;
    bool attached;
    int refs, running;
    bool exited;          /* the waiter saw the exit: a stop must not signal a pid reused */
    int64_t code;
    char* in;             /* bytes for its standard input, not yet written */
    size_t in_len, in_cap;
    bool in_closing, in_writing, in_broken;
    gardel_cond in_cv;
#ifdef _WIN32
    void* process;
    void* job;
    void* in_w;
    void* out_r;
    void* err_r;
#else
    pid_t pid;
    int in_w, out_r, err_r;
#endif
    struct gardel_proc* next;
};
static struct gardel_proc* gardel_procs = NULL;
static uint64_t gardel_next_proc = 1;
static bool gardel_attached = false;   /* under the lock */
static bool gardel_reader_in_gate = false, gardel_reader_parked = false;
static gardel_cond gardel_park_cv = GARDEL_COND_INIT;

#ifdef _WIN32
#define GARDEL_FILE_ERR_INVALID 87      /* ERROR_INVALID_PARAMETER */
#define GARDEL_FILE_ERR_TOO_BIG 223     /* ERROR_FILE_TOO_LARGE */
#define GARDEL_FILE_ERR_BUSY 170        /* ERROR_BUSY */
#define GARDEL_FILE_ERR_NOT_PROGRAM 193 /* ERROR_BAD_EXE_FORMAT */
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) int __stdcall MultiByteToWideChar(unsigned int, unsigned long, const char*, int, wchar_t*, int);
__declspec(dllimport) int __stdcall WideCharToMultiByte(unsigned int, unsigned long, const wchar_t*, int, char*, int, const char*, int*);
__declspec(dllimport) void* __stdcall CreateFileW(const wchar_t*, unsigned long, unsigned long, void*, unsigned long, unsigned long, void*);
__declspec(dllimport) int __stdcall WriteFile(void*, const void*, unsigned long, unsigned long*, void*);
__declspec(dllimport) void* __stdcall FindFirstFileW(const wchar_t*, void*);
__declspec(dllimport) int __stdcall FindNextFileW(void*, void*);
__declspec(dllimport) int __stdcall FindClose(void*);
__declspec(dllimport) int __stdcall CreatePipe(void**, void**, void*, unsigned long);
__declspec(dllimport) int __stdcall SetHandleInformation(void*, unsigned long, unsigned long);
__declspec(dllimport) int __stdcall CreateProcessW(const wchar_t*, wchar_t*, void*, void*, int, unsigned long, void*, const wchar_t*, void*, void*);
__declspec(dllimport) int __stdcall GetExitCodeProcess(void*, unsigned long*);
__declspec(dllimport) int __stdcall TerminateProcess(void*, unsigned int);
__declspec(dllimport) unsigned long __stdcall ResumeThread(void*);
__declspec(dllimport) void* __stdcall CreateJobObjectW(void*, const wchar_t*);
__declspec(dllimport) int __stdcall SetInformationJobObject(void*, int, void*, unsigned long);
__declspec(dllimport) int __stdcall AssignProcessToJobObject(void*, void*);
__declspec(dllimport) int __stdcall TerminateJobObject(void*, unsigned int);
__declspec(dllimport) unsigned long __stdcall GetEnvironmentVariableW(const wchar_t*, wchar_t*, unsigned long);
__declspec(dllimport) unsigned long __stdcall SearchPathW(const wchar_t*, const wchar_t*, const wchar_t*, unsigned long, wchar_t*, wchar_t**);
__declspec(dllimport) int __stdcall FlushFileBuffers(void*);
__declspec(dllimport) int __stdcall MoveFileExW(const wchar_t*, const wchar_t*, unsigned long);
__declspec(dllimport) int __stdcall DeleteFileW(const wchar_t*);
#include <wchar.h>
#define GARDEL_NO_HANDLE ((void*)(intptr_t)-1)
static void* gardel_reader_wake = NULL; /* auto-reset: a RunAttached wants the reader parked */

/* UTF-8 to UTF-16, NULL-terminated, or NULL when it is not valid UTF-8. */
static wchar_t* gardel_wide(const char* s, size_t n) {
    int len = n == 0 ? 0 : MultiByteToWideChar(65001, 8 /* MB_ERR_INVALID_CHARS */, s, (int)n, NULL, 0);
    if (n > 0 && len <= 0) return NULL;
    wchar_t* w = (wchar_t*)malloc(sizeof(wchar_t) * ((size_t)len + 1));
    if (!w) abort();
    if (len > 0) MultiByteToWideChar(65001, 8, s, (int)n, w, len);
    w[len] = 0;
    return w;
}

static int64_t gardel_os_read(const char* path, size_t path_len, char** out, size_t* out_len) {
    wchar_t* w = gardel_wide(path, path_len);
    if (!w) return GARDEL_FILE_ERR_INVALID;
    /* GENERIC_READ, any sharing, OPEN_EXISTING */
    void* h = CreateFileW(w, 0x80000000UL, 7, NULL, 3, 0x80, NULL);
    free(w);
    if (h == GARDEL_NO_HANDLE) return (int64_t)GetLastError();
    size_t cap = 65536, len = 0;
    char* buf = (char*)malloc(cap);
    if (!buf) abort();
    int64_t err = 0;
    for (;;) {
        if (len == cap) {
            if (cap > GARDEL_FILE_MAX) { err = GARDEL_FILE_ERR_TOO_BIG; break; }
            cap *= 2;
            buf = (char*)realloc(buf, cap);
            if (!buf) abort();
        }
        unsigned long got = 0;
        size_t want = cap - len > 0x40000000 ? 0x40000000 : cap - len;
        if (!ReadFile(h, buf + len, (unsigned long)want, &got, NULL)) { err = (int64_t)GetLastError(); break; }
        if (got == 0) break;
        len += got;
    }
    CloseHandle(h);
    if (!err && len > GARDEL_FILE_MAX) err = GARDEL_FILE_ERR_TOO_BIG;
    if (err) { free(buf); return err; }
    *out = buf;
    *out_len = len;
    return 0;
}

/* `data` written to `tmp`, a new file beside `path`, flushed, then renamed over `path`: a crash
   leaves the old file or the new one, never a part. `tmp` is removed when anything fails. */
static int64_t gardel_os_write(const char* path, size_t path_len, const char* tmp, size_t tmp_len, const char* data, size_t n) {
    wchar_t* w = gardel_wide(path, path_len);
    wchar_t* wt = gardel_wide(tmp, tmp_len);
    if (!w || !wt || path_len == 0 || memchr(path, 0, path_len)) { free(w); free(wt); return GARDEL_FILE_ERR_INVALID; }
    /* GENERIC_WRITE, no sharing, CREATE_NEW */
    void* h = CreateFileW(wt, 0x40000000UL, 0, NULL, 1, 0x80, NULL);
    if (h == GARDEL_NO_HANDLE) { int64_t e = (int64_t)GetLastError(); free(w); free(wt); return e; }
    int64_t err = 0;
    while (n > 0 && !err) {
        unsigned long put = 0;
        if (!WriteFile(h, data, (unsigned long)(n > 0x40000000 ? 0x40000000 : n), &put, NULL) || put == 0) err = (int64_t)GetLastError();
        data += put;
        n -= put;
    }
    if (!err && !FlushFileBuffers(h)) err = (int64_t)GetLastError();
    CloseHandle(h);
    /* MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH */
    if (!err && !MoveFileExW(wt, w, 1 | 8)) err = (int64_t)GetLastError();
    if (err) DeleteFileW(wt);
    free(w);
    free(wt);
    return err;
}

/* WIN32_FIND_DATAW: FILETIME is two DWORDs, so it packs at 4. */
struct gardel_find_data {
    unsigned long attrs;
    unsigned long times[6];
    unsigned long size_high, size_low, reserved0, reserved1;
    wchar_t name[260];
    wchar_t alt[14];
    unsigned long mac[2];
};

static void gardel_dir_add(struct gardel_dir_item** items, size_t* n, size_t* cap, const char* name, size_t len, bool is_dir);

static int64_t gardel_os_list(const char* path, size_t path_len, struct gardel_dir_item** items, size_t* count) {
    char* pattern = (char*)malloc(path_len + 3);
    if (!pattern) abort();
    memcpy(pattern, path, path_len);
    size_t n = path_len;
    if (n > 0 && pattern[n - 1] != '/' && pattern[n - 1] != '\\') pattern[n++] = '\\';
    pattern[n++] = '*';
    wchar_t* w = gardel_wide(pattern, n);
    free(pattern);
    if (!w) return GARDEL_FILE_ERR_INVALID;
    struct gardel_find_data d;
    void* h = FindFirstFileW(w, &d);
    free(w);
    size_t cap = 0;
    *items = NULL;
    *count = 0;
    if (h == GARDEL_NO_HANDLE) {
        unsigned long e = GetLastError();
        return e == 2 /* ERROR_FILE_NOT_FOUND: no entry at all */ ? 0 : (int64_t)e;
    }
    do {
        const wchar_t* nm = d.name;
        if (nm[0] == L'.' && (nm[1] == 0 || (nm[1] == L'.' && nm[2] == 0))) continue;
        /* A name with a lone surrogate has no UTF-8 form, so it is left out. */
        int len = WideCharToMultiByte(65001, 0x80 /* WC_ERR_INVALID_CHARS */, nm, -1, NULL, 0, NULL, NULL);
        if (len <= 1) continue;
        char* u = (char*)malloc((size_t)len);
        if (!u) abort();
        WideCharToMultiByte(65001, 0x80, nm, -1, u, len, NULL, NULL);
        gardel_dir_add(items, count, &cap, u, (size_t)len - 1, (d.attrs & 0x10) != 0);
        free(u);
    } while (FindNextFileW(h, &d));
    unsigned long e = GetLastError();
    FindClose(h);
    if (e != 18 /* ERROR_NO_MORE_FILES */) {
        for (size_t i = 0; i < *count; i++) free((*items)[i].name);
        free(*items);
        *items = NULL;
        *count = 0;
        return (int64_t)e;
    }
    return 0;
}

/* `a` quoted for CommandLineToArgvW and the C runtime: backslashes are literal except before a
   quote, where they are doubled. */
static void gardel_cmd_put(wchar_t** out, size_t* len, size_t* cap, wchar_t c) {
    *out = (wchar_t*)gardel_grow(*out, cap, *len + 2, sizeof(wchar_t));
    (*out)[(*len)++] = c;
    (*out)[*len] = 0;
}
static void gardel_cmd_quote(wchar_t** out, size_t* len, size_t* cap, const wchar_t* a) {
    if (*len > 0) gardel_cmd_put(out, len, cap, L' ');
    gardel_cmd_put(out, len, cap, L'"');
    for (size_t i = 0;; i++) {
        size_t bs = 0;
        while (a[i] == L'\\') { bs++; i++; }
        if (a[i] == 0) {
            for (size_t k = 0; k < 2 * bs; k++) gardel_cmd_put(out, len, cap, L'\\');
            break;
        }
        size_t keep = a[i] == L'"' ? 2 * bs + 1 : bs;
        for (size_t k = 0; k < keep; k++) gardel_cmd_put(out, len, cap, L'\\');
        gardel_cmd_put(out, len, cap, a[i]);
    }
    gardel_cmd_put(out, len, cap, L'"');
}

struct gardel_startup {
    unsigned long cb;
    wchar_t* reserved;
    wchar_t* desktop;
    wchar_t* title;
    unsigned long x, y, x_size, y_size, x_chars, y_chars, fill, flags;
    unsigned short show, reserved2;
    unsigned char* reserved3;
    void* std_in;
    void* std_out;
    void* std_err;
};
struct gardel_proc_info { void* process; void* thread; unsigned long pid, tid; };
struct gardel_security { unsigned long size; void* descriptor; int inherit; };
/* JOBOBJECT_EXTENDED_LIMIT_INFORMATION */
struct gardel_job_limits {
    long long user_time, job_time;
    unsigned long flags;
    size_t min_ws, max_ws;
    unsigned long active;
    size_t affinity;
    unsigned long priority, scheduling;
    unsigned long long io[6];
    size_t process_memory, job_memory, peak_process, peak_job;
};

/* A pipe whose `child` end the child inherits and whose other end it does not. */
static bool gardel_os_pipe(void** read_end, void** write_end, bool child_reads) {
    struct gardel_security sa = { sizeof sa, NULL, 1 };
    if (!CreatePipe(read_end, write_end, &sa, 0)) return false;
    SetHandleInformation(child_reads ? *write_end : *read_end, 1 /* HANDLE_FLAG_INHERIT */, 0);
    return true;
}

/* Starts `p`: 0, or the OS error. The program is looked up in PATH only, never in the current
   directory first, and a batch file is refused: cmd.exe would read its arguments as a shell
   does (decision 11). */
static int64_t gardel_os_spawn(struct gardel_proc* p, const char* program, size_t program_len, const char** args, const size_t* arg_lens, size_t n, const char* dir, size_t dir_len) {
    wchar_t* wprog = gardel_wide(program, program_len);
    if (!wprog || program_len == 0 || wcschr(wprog, L'"')) { free(wprog); return GARDEL_FILE_ERR_INVALID; }
    wchar_t full[32768];
    if (!wcschr(wprog, L'\\') && !wcschr(wprog, L'/') && !wcschr(wprog, L':')) {
        wchar_t* path = (wchar_t*)malloc(sizeof(wchar_t) * 32768);
        if (!path) abort();
        unsigned long got = GetEnvironmentVariableW(L"PATH", path, 32768);
        unsigned long found = got > 0 && got < 32768 ? SearchPathW(path, wprog, L".exe", 32768, full, NULL) : 0;
        free(path);
        if (found == 0 || found >= 32768) { free(wprog); return 2; /* ERROR_FILE_NOT_FOUND */ }
    } else {
        size_t l = wcslen(wprog);
        if (l + 5 >= 32768) { free(wprog); return GARDEL_FILE_ERR_INVALID; }
        memcpy(full, wprog, sizeof(wchar_t) * (l + 1));
        const wchar_t* base = full;
        for (const wchar_t* q = full; *q; q++) if (*q == L'\\' || *q == L'/' || *q == L':') base = q + 1;
        if (!wcschr(base, L'.')) wcscat(full, L".exe");
    }
    size_t fl = wcslen(full);
    if (fl >= 4 && (_wcsicmp(full + fl - 4, L".bat") == 0 || _wcsicmp(full + fl - 4, L".cmd") == 0)) {
        free(wprog);
        return GARDEL_FILE_ERR_NOT_PROGRAM;
    }
    wchar_t* cmd = NULL;
    size_t len = 0, cap = 0;
    gardel_cmd_quote(&cmd, &len, &cap, wprog);
    free(wprog);
    for (size_t i = 0; i < n; i++) {
        wchar_t* wa = gardel_wide(args[i], arg_lens[i]);
        if (!wa || wcslen(wa) != (size_t)MultiByteToWideChar(65001, 0, args[i], (int)arg_lens[i], NULL, 0)) {
            free(wa);
            free(cmd);
            return GARDEL_FILE_ERR_INVALID; /* not UTF-8, or a NUL inside */
        }
        gardel_cmd_quote(&cmd, &len, &cap, wa);
        free(wa);
    }
    if (len >= 32767) { free(cmd); return GARDEL_FILE_ERR_INVALID; }
    wchar_t* wdir = NULL;
    if (dir_len > 0) {
        wdir = gardel_wide(dir, dir_len);
        if (!wdir) { free(cmd); return GARDEL_FILE_ERR_INVALID; }
    }
    struct gardel_startup si;
    memset(&si, 0, sizeof si);
    si.cb = sizeof si;
    void* in_r = NULL; void* out_w = NULL; void* err_w = NULL;
    p->in_w = p->out_r = p->err_r = NULL;
    unsigned long flags = 4; /* CREATE_SUSPENDED, until it is in its job */
    if (!p->attached) {
        if (!gardel_os_pipe(&in_r, &p->in_w, true) || !gardel_os_pipe(&p->out_r, &out_w, false) || !gardel_os_pipe(&p->err_r, &err_w, false)) {
            int64_t e = (int64_t)GetLastError();
            void* hs[6] = { in_r, p->in_w, p->out_r, out_w, p->err_r, err_w };
            for (int i = 0; i < 6; i++) if (hs[i]) CloseHandle(hs[i]);
            free(cmd); free(wdir);
            return e;
        }
        si.flags = 0x100; /* STARTF_USESTDHANDLES */
        si.std_in = in_r;
        si.std_out = out_w;
        si.std_err = err_w;
        flags |= 0x08000000; /* CREATE_NO_WINDOW: no console, so Ctrl-C and the screen stay ours */
    }
    struct gardel_proc_info pi;
    int made = CreateProcessW(full, cmd, NULL, NULL, 1, flags, NULL, wdir, &si, &pi);
    int64_t e = made ? 0 : (int64_t)GetLastError();
    free(cmd);
    free(wdir);
    if (!p->attached) {
        CloseHandle(in_r);
        CloseHandle(out_w);
        CloseHandle(err_w);
        if (!made) {
            CloseHandle(p->in_w);
            CloseHandle(p->out_r);
            CloseHandle(p->err_r);
            p->in_w = p->out_r = p->err_r = NULL;
        }
    }
    if (!made) return e;
    /* A job ends what the child started, too, when it is stopped or when this program ends. */
    p->job = CreateJobObjectW(NULL, NULL);
    if (p->job) {
        struct gardel_job_limits lim;
        memset(&lim, 0, sizeof lim);
        lim.flags = 0x2000; /* JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE */
        if (!SetInformationJobObject(p->job, 9, &lim, sizeof lim) || !AssignProcessToJobObject(p->job, pi.process)) {
            CloseHandle(p->job);
            p->job = NULL;
        }
    }
    ResumeThread(pi.thread);
    CloseHandle(pi.thread);
    p->process = pi.process;
    return 0;
}

static int64_t gardel_os_wait(struct gardel_proc* p) {
    WaitForSingleObject(p->process, 0xFFFFFFFFUL);
    unsigned long code = 0;
    GetExitCodeProcess(p->process, &code);
    return (int64_t)code;
}
/* Under the lock, before the exit is seen: the handle stays valid until the record is freed. */
static void gardel_os_stop(struct gardel_proc* p) {
    if (p->job) TerminateJobObject(p->job, 1);
    else TerminateProcess(p->process, 1);
}
static long gardel_os_pipe_read(void* h, char* buf, size_t n) {
    unsigned long got = 0;
    if (!ReadFile(h, buf, (unsigned long)n, &got, NULL)) return -1;
    return (long)got;
}
static bool gardel_os_pipe_write(void* h, const char* buf, size_t n) {
    while (n > 0) {
        unsigned long put = 0;
        if (!WriteFile(h, buf, (unsigned long)(n > 0x40000000 ? 0x40000000 : n), &put, NULL) || put == 0) return false;
        buf += put;
        n -= put;
    }
    return true;
}
static void gardel_os_close(void* h) { if (h) CloseHandle(h); }
static void gardel_os_free(struct gardel_proc* p) {
    gardel_os_close(p->process);
    gardel_os_close(p->job);
}
#define GARDEL_PROC_NO_PIPE NULL
static void gardel_reader_wake_now(void) {
    if (gardel_reader_wake) SetEvent(gardel_reader_wake);
}
/* Until a key can be read, with the reader parked while a RunAttached child has the terminal.
   A console and a pipe can be watched; a file is read as it is, as its bytes are all there. */
static bool gardel_reader_gate(void) {
    for (;;) {
        gardel_lock();
        if (gardel_attached) {
            gardel_reader_parked = true;
            gardel_cond_wake(&gardel_park_cv);
            while (gardel_attached) gardel_cond_wait(&gardel_park_cv, UINT64_MAX);
            gardel_reader_parked = false;
        }
        gardel_reader_in_gate = true;
        gardel_unlock();
        gardel_term_set_raw(true);
        unsigned long r = 0;
        if (gardel_term_is_console && gardel_reader_wake) {
            void* both[2] = { gardel_term_in, gardel_reader_wake };
            r = WaitForMultipleObjects(2, both, 0, 0xFFFFFFFFUL);
        } else if (gardel_reader_wake && GetFileType(gardel_term_in) == 3 /* FILE_TYPE_PIPE */) {
            /* A pipe cannot be waited on with an event: look for bytes, or a wake, every 5 ms. */
            for (;;) {
                unsigned long avail = 0;
                if (!PeekNamedPipe(gardel_term_in, NULL, 0, NULL, &avail, NULL) || avail > 0) break;
                if (WaitForSingleObject(gardel_reader_wake, 5) == 0) { r = 1; break; }
            }
        }
        gardel_lock();
        gardel_reader_in_gate = false;
        gardel_unlock();
        if (r != 1) return true;
    }
}
static void gardel_reader_gate_open(void) {
    if (!gardel_reader_wake) gardel_reader_wake = CreateEventA(NULL, 0, 0, NULL);
}
#else
#include <dirent.h>
#include <sys/stat.h>
#include <sys/wait.h>
#define GARDEL_FILE_ERR_INVALID EINVAL
#define GARDEL_FILE_ERR_TOO_BIG EFBIG
#define GARDEL_FILE_ERR_BUSY EBUSY
#define GARDEL_FILE_ERR_NOT_PROGRAM ENOEXEC
static int gardel_reader_wake_pipe[2] = {-1, -1};

/* A path as a C string, or NULL when it holds a NUL. */
static char* gardel_c_text(const char* s, size_t n) {
    if (memchr(s, 0, n)) return NULL;
    char* c = (char*)malloc(n + 1);
    if (!c) abort();
    memcpy(c, s, n);
    c[n] = 0;
    return c;
}

static int64_t gardel_os_read(const char* path, size_t path_len, char** out, size_t* out_len) {
    char* c = gardel_c_text(path, path_len);
    if (!c) return GARDEL_FILE_ERR_INVALID;
    int fd = open(c, O_RDONLY | O_CLOEXEC);
    free(c);
    if (fd < 0) return errno;
    struct stat st;
    if (fstat(fd, &st) == 0 && S_ISDIR(st.st_mode)) { close(fd); return EISDIR; }
    size_t cap = 65536, len = 0;
    char* buf = (char*)malloc(cap);
    if (!buf) abort();
    int64_t err = 0;
    for (;;) {
        if (len == cap) {
            if (cap > GARDEL_FILE_MAX) { err = GARDEL_FILE_ERR_TOO_BIG; break; }
            cap *= 2;
            buf = (char*)realloc(buf, cap);
            if (!buf) abort();
        }
        ssize_t got = read(fd, buf + len, cap - len);
        if (got < 0 && errno == EINTR) continue;
        if (got < 0) { err = errno; break; }
        if (got == 0) break;
        len += (size_t)got;
    }
    close(fd);
    if (!err && len > GARDEL_FILE_MAX) err = GARDEL_FILE_ERR_TOO_BIG;
    if (err) { free(buf); return err; }
    *out = buf;
    *out_len = len;
    return 0;
}

/* As on Windows: a new file beside `path`, with the mode of the file it replaces, written,
   synced and renamed over it, then the directory synced, so the rename lasts. */
static int64_t gardel_os_write(const char* path, size_t path_len, const char* tmp, size_t tmp_len, const char* data, size_t n) {
    char* c = path_len ? gardel_c_text(path, path_len) : NULL;
    char* t = gardel_c_text(tmp, tmp_len);
    if (!c || !t) { free(c); free(t); return GARDEL_FILE_ERR_INVALID; }
    int fd = open(t, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0666);
    if (fd < 0) { int e = errno; free(c); free(t); return e; }
    int64_t err = 0;
    struct stat st;
    if (stat(c, &st) == 0) {
        if (S_ISDIR(st.st_mode)) err = EISDIR;
        else if (fchmod(fd, st.st_mode & 07777) != 0) err = errno;
    }
    while (n > 0 && !err) {
        ssize_t put = write(fd, data, n);
        if (put < 0 && errno == EINTR) continue;
        if (put <= 0) { err = put < 0 ? errno : EIO; break; }
        data += put;
        n -= (size_t)put;
    }
    if (!err && fsync(fd) != 0) err = errno;
    if (close(fd) != 0 && !err) err = errno;
    if (!err && rename(t, c) != 0) err = errno;
    if (err) {
        unlink(t);
    } else {
        char* slash = strrchr(c, '/');
        if (slash == c) slash[1] = 0;
        else if (slash) *slash = 0;
        int d = open(slash ? c : ".", O_RDONLY | O_CLOEXEC);
        if (d >= 0) { fsync(d); close(d); }
    }
    free(c);
    free(t);
    return err;
}

static void gardel_dir_add(struct gardel_dir_item** items, size_t* n, size_t* cap, const char* name, size_t len, bool is_dir);

static int64_t gardel_os_list(const char* path, size_t path_len, struct gardel_dir_item** items, size_t* count) {
    char* c = gardel_c_text(path, path_len);
    if (!c) return GARDEL_FILE_ERR_INVALID;
    DIR* d = opendir(c);
    free(c);
    *items = NULL;
    *count = 0;
    if (!d) return errno;
    size_t cap = 0;
    for (;;) {
        errno = 0;
        struct dirent* e = readdir(d);
        if (!e) break;
        const char* nm = e->d_name;
        if (nm[0] == '.' && (nm[1] == 0 || (nm[1] == '.' && nm[2] == 0))) continue;
        bool is_dir = false;
#ifdef DT_DIR
        if (e->d_type == DT_DIR) is_dir = true;
        else if (e->d_type == DT_UNKNOWN || e->d_type == DT_LNK)
#endif
        {
            struct stat st;
            is_dir = fstatat(dirfd(d), nm, &st, 0) == 0 && S_ISDIR(st.st_mode);
        }
        gardel_dir_add(items, count, &cap, nm, strlen(nm), is_dir);
    }
    int err = errno;
    closedir(d);
    if (err) {
        for (size_t i = 0; i < *count; i++) free((*items)[i].name);
        free(*items);
        *items = NULL;
        *count = 0;
        return err;
    }
    return 0;
}

static bool gardel_os_cloexec_pipe(int fds[2]) {
    if (pipe(fds) != 0) return false;
    fcntl(fds[0], F_SETFD, FD_CLOEXEC);
    fcntl(fds[1], F_SETFD, FD_CLOEXEC);
    return true;
}

/* Starts `p`: 0, or the error. A captured child gets a process group of its own, so Ctrl-C at
   the terminal is not its, and a stop ends what it started too. An attached one stays in the
   terminal's foreground group. `exec` failing comes back through a pipe closed on exec. */
static int64_t gardel_os_spawn(struct gardel_proc* p, const char* program, size_t program_len, const char** args, const size_t* arg_lens, size_t n, const char* dir, size_t dir_len) {
    char** argv = (char**)calloc(n + 2, sizeof(char*));
    if (!argv) abort();
    char* cdir = NULL;
    int64_t bad = 0;
    argv[0] = program_len ? gardel_c_text(program, program_len) : NULL;
    if (!argv[0]) bad = program_len ? GARDEL_FILE_ERR_INVALID : ENOENT;
    /* A relative path with a slash names a file from this program's directory, as on Windows,
       not from `dir`, which the child enters first. */
    if (!bad && dir_len > 0 && argv[0][0] != '/' && strchr(argv[0], '/')) {
        char here[4096];
        if (getcwd(here, sizeof here)) {
            size_t hl = strlen(here), pl = strlen(argv[0]);
            char* abs = (char*)malloc(hl + pl + 2);
            if (!abs) abort();
            memcpy(abs, here, hl);
            abs[hl] = '/';
            memcpy(abs + hl + 1, argv[0], pl + 1);
            free(argv[0]);
            argv[0] = abs;
        }
    }
    for (size_t i = 0; i < n && !bad; i++) {
        argv[i + 1] = gardel_c_text(args[i], arg_lens[i]);
        if (!argv[i + 1]) bad = GARDEL_FILE_ERR_INVALID;
    }
    if (!bad && dir_len > 0 && !(cdir = gardel_c_text(dir, dir_len))) bad = GARDEL_FILE_ERR_INVALID;
    int in[2] = {-1, -1}, out[2] = {-1, -1}, er[2] = {-1, -1}, ch[2] = {-1, -1};
    if (!bad && !p->attached && (!gardel_os_cloexec_pipe(in) || !gardel_os_cloexec_pipe(out) || !gardel_os_cloexec_pipe(er))) bad = errno;
    if (!bad && !gardel_os_cloexec_pipe(ch)) bad = errno;
    pid_t pid = -1;
    if (!bad) {
        pid = fork();
        if (pid < 0) bad = errno;
    }
    if (pid == 0) {
        if (!p->attached) {
            setpgid(0, 0);
            dup2(in[0], 0);
            dup2(out[1], 1);
            dup2(er[1], 2);
        }
        struct sigaction dfl;
        memset(&dfl, 0, sizeof dfl);
        dfl.sa_handler = SIG_DFL;
        sigemptyset(&dfl.sa_mask);
        sigaction(SIGPIPE, &dfl, NULL);
        sigaction(SIGINT, &dfl, NULL);
        sigaction(SIGTERM, &dfl, NULL);
        sigaction(SIGWINCH, &dfl, NULL);
        sigset_t none;
        sigemptyset(&none);
        sigprocmask(SIG_SETMASK, &none, NULL);
        int e = 0;
        if (cdir && chdir(cdir) != 0) e = errno;
        if (!e) {
            execvp(argv[0], argv);
            e = errno;
        }
        ssize_t ignored = write(ch[1], &e, sizeof e);
        (void)ignored;
        _exit(127);
    }
    int fds[8] = { in[0], out[1], er[1], ch[1] };
    for (int i = 0; i < 4; i++) if (fds[i] >= 0) close(fds[i]);
    if (!bad) {
        int e = 0;
        ssize_t got;
        while ((got = read(ch[0], &e, sizeof e)) < 0 && errno == EINTR) {}
        if (got == (ssize_t)sizeof e) {
            bad = e ? e : ENOENT;
            int st;
            while (waitpid(pid, &st, 0) < 0 && errno == EINTR) {}
        }
    }
    if (ch[0] >= 0) close(ch[0]);
    for (size_t i = 0; i <= n; i++) free(argv[i]);
    free(argv);
    free(cdir);
    if (bad) {
        int rest[3] = { in[1], out[0], er[0] };
        for (int i = 0; i < 3; i++) if (rest[i] >= 0) close(rest[i]);
        return bad;
    }
    p->pid = pid;
    p->in_w = in[1];
    p->out_r = out[0];
    p->err_r = er[0];
    return 0;
}

/* The exit is seen without reaping, so that a stop under the lock cannot reach a pid reused. */
static int64_t gardel_os_wait(struct gardel_proc* p) {
    siginfo_t info;
    memset(&info, 0, sizeof info);
    while (waitid(P_PID, (id_t)p->pid, &info, WEXITED | WNOWAIT) < 0 && errno == EINTR) {}
    gardel_lock();
    p->exited = true;
    gardel_unlock();
    int st = 0;
    while (waitpid(p->pid, &st, 0) < 0 && errno == EINTR) {}
    if (WIFEXITED(st)) return WEXITSTATUS(st);
    if (WIFSIGNALED(st)) return 128 + WTERMSIG(st);
    return 255;
}
static void gardel_os_stop(struct gardel_proc* p) {
    if (!p->exited) kill(p->attached ? p->pid : -p->pid, SIGKILL);
}
static long gardel_os_pipe_read(int fd, char* buf, size_t n) {
    ssize_t got;
    while ((got = read(fd, buf, n)) < 0 && errno == EINTR) {}
    return (long)got;
}
static bool gardel_os_pipe_write(int fd, const char* buf, size_t n) {
    while (n > 0) {
        ssize_t put = write(fd, buf, n);
        if (put < 0 && errno == EINTR) continue;
        if (put <= 0) return false;
        buf += put;
        n -= (size_t)put;
    }
    return true;
}
static void gardel_os_close(int fd) { if (fd >= 0) close(fd); }
static void gardel_os_free(struct gardel_proc* p) { (void)p; }
#define GARDEL_PROC_NO_PIPE (-1)
static void gardel_reader_wake_now(void) {
    if (gardel_reader_wake_pipe[1] >= 0) {
        char c = 1;
        ssize_t ignored = write(gardel_reader_wake_pipe[1], &c, 1);
        (void)ignored;
    }
}
static bool gardel_reader_gate(void) {
    for (;;) {
        gardel_lock();
        if (gardel_attached) {
            gardel_reader_parked = true;
            gardel_cond_wake(&gardel_park_cv);
            while (gardel_attached) gardel_cond_wait(&gardel_park_cv, UINT64_MAX);
            gardel_reader_parked = false;
        }
        gardel_reader_in_gate = true;
        gardel_unlock();
        gardel_term_set_raw(true);
        bool woken = false;
        if (gardel_reader_wake_pipe[0] >= 0) {
            struct pollfd p[2] = { { 0, POLLIN, 0 }, { gardel_reader_wake_pipe[0], POLLIN, 0 } };
            while (poll(p, 2, -1) < 0 && errno == EINTR) {}
            if (p[1].revents & POLLIN) {
                char buf[16];
                while (read(gardel_reader_wake_pipe[0], buf, sizeof buf) > 0) {}
                woken = true;
            }
        }
        gardel_lock();
        gardel_reader_in_gate = false;
        gardel_unlock();
        if (!woken) return true;
    }
}
static void gardel_reader_gate_open(void) {
    if (gardel_reader_wake_pipe[0] >= 0) return;
    if (!gardel_os_cloexec_pipe(gardel_reader_wake_pipe)) abort();
    fcntl(gardel_reader_wake_pipe[0], F_SETFL, O_NONBLOCK);
    fcntl(gardel_reader_wake_pipe[1], F_SETFL, O_NONBLOCK);
}
#endif

static void gardel_dir_add(struct gardel_dir_item** items, size_t* n, size_t* cap, const char* name, size_t len, bool is_dir) {
    *items = (struct gardel_dir_item*)gardel_grow(*items, cap, *n + 1, sizeof **items);
    char* copy = (char*)malloc(len + 1);
    if (!copy) abort();
    memcpy(copy, name, len);
    copy[len] = 0;
    (*items)[(*n)++] = (struct gardel_dir_item){ copy, len, is_dir };
}

/* Byte order of the names, which is code point order: the same listing on every system. */
static int gardel_dir_order(const void* a, const void* b) {
    const struct gardel_dir_item* x = (const struct gardel_dir_item*)a;
    const struct gardel_dir_item* y = (const struct gardel_dir_item*)b;
    size_t n = x->len < y->len ? x->len : y->len;
    int c = memcmp(x->name, y->name, n);
    if (c != 0) return c;
    return x->len < y->len ? -1 : x->len > y->len ? 1 : 0;
}

static void gardel_dir_free(struct gardel_host_event* ev) {
    for (size_t i = 0; i < ev->item_count; i++) free(ev->items[i].name);
    free(ev->items);
    ev->items = NULL;
    ev->item_count = 0;
}

static struct gardel_answer* gardel_answer_new(enum gardel_host_kind kind) {
    struct gardel_answer* a = (struct gardel_answer*)calloc(1, sizeof *a);
    if (!a) abort();
    a->ev.kind = kind;
    return a;
}
static void gardel_answer_post(struct gardel_answer* a) {
    gardel_lock();
    gardel_answer_push(a);
    gardel_unlock();
}

static void gardel_trace_text(const char* s, size_t n) {
    gardel_trace_bytes((GardelBytes){ (const uint8_t*)s, (uint64_t)n, NULL });
}

/* ReadFile and ListDir: one helper thread each, as a file system can block for long. */
struct gardel_file_job { uint64_t tag; bool list; char* path; size_t len; };
static void gardel_file_main(void* arg) {
    struct gardel_file_job* j = (struct gardel_file_job*)arg;
    struct gardel_answer* a = gardel_answer_new(j->list ? GARDEL_HOST_DIR_LISTED : GARDEL_HOST_FILE_READ);
    a->ev.tag = j->tag;
    if (j->list) {
        a->ev.err = gardel_os_list(j->path, j->len, &a->ev.items, &a->ev.item_count);
        if (a->ev.item_count > 1) qsort(a->ev.items, a->ev.item_count, sizeof *a->ev.items, gardel_dir_order);
    } else {
        a->ev.err = gardel_os_read(j->path, j->len, &a->ev.buf, &a->ev.buf_len);
    }
    a->ev.ok = a->ev.err == 0;
    free(j->path);
    free(j);
    gardel_answer_post(a);
}
static void gardel_host_file(uint64_t tag, GardelString path, bool list) {
    if (gardel_host_script) {
        fprintf(stderr, "file: %s %llu ", list ? "ListDir" : "ReadFile", (unsigned long long)tag);
        gardel_trace_text(path.data, path.len);
        fprintf(stderr, "\n");
        return;
    }
    struct gardel_file_job* j = (struct gardel_file_job*)malloc(sizeof *j);
    if (!j) abort();
    j->tag = tag;
    j->list = list;
    /* An empty path is the current directory, for a listing. */
    const char* p = path.len == 0 && list ? "." : path.data;
    j->len = path.len == 0 && list ? 1 : (size_t)path.len;
    j->path = (char*)malloc(j->len + 1);
    if (!j->path) abort();
    if (j->len) memcpy(j->path, p, j->len);
    j->path[j->len] = 0;
    gardel_thread_run(gardel_file_main, j);
}

/* WriteFile: on a helper thread too, with its own copy of the bytes. The new file is named
   after the path, this process and a count, so two writes never share one. */
struct gardel_write_job { uint64_t tag; char* path; size_t len; char* tmp; size_t tmp_len; char* data; size_t n; };
static uint64_t gardel_writes_made = 0;
static void gardel_write_main(void* arg) {
    struct gardel_write_job* j = (struct gardel_write_job*)arg;
    struct gardel_answer* a = gardel_answer_new(GARDEL_HOST_FILE_WRITTEN);
    a->ev.tag = j->tag;
    a->ev.err = gardel_os_write(j->path, j->len, j->tmp, j->tmp_len, j->data, j->n);
    a->ev.ok = a->ev.err == 0;
    a->ev.id = (uint64_t)j->n;
    free(j->path);
    free(j->tmp);
    free(j->data);
    free(j);
    gardel_answer_post(a);
}
static void gardel_host_write_file(uint64_t tag, GardelString path, GardelBytes data) {
    if (gardel_host_script) {
        fprintf(stderr, "file: WriteFile %llu ", (unsigned long long)tag);
        gardel_trace_text(path.data, path.len);
        fprintf(stderr, " ");
        gardel_trace_bytes(data);
        fprintf(stderr, "\n");
        return;
    }
    struct gardel_write_job* j = (struct gardel_write_job*)malloc(sizeof *j);
    if (!j) abort();
    j->tag = tag;
    j->len = (size_t)path.len;
    j->path = (char*)malloc(j->len + 1);
    j->n = (size_t)data.len;
    j->data = (char*)malloc(j->n + 1);
    char suffix[64];
#ifdef _WIN32
    unsigned long pid = GetCurrentProcessId();
#else
    unsigned long pid = (unsigned long)getpid();
#endif
    int sl = snprintf(suffix, sizeof suffix, ".gardel-%lu-%llu.tmp", pid, (unsigned long long)++gardel_writes_made);
    j->tmp_len = j->len + (size_t)sl;
    j->tmp = (char*)malloc(j->tmp_len + 1);
    if (!j->path || !j->data || !j->tmp) abort();
    if (j->len) memcpy(j->path, path.data, j->len);
    j->path[j->len] = 0;
    if (j->n) memcpy(j->data, data.data, j->n);
    if (j->len) memcpy(j->tmp, path.data, j->len);
    memcpy(j->tmp + j->len, suffix, (size_t)sl + 1);
    gardel_thread_run(gardel_write_main, j);
}

/* Processes. */
static struct gardel_proc* gardel_proc_find(uint64_t id) {
    for (struct gardel_proc* p = gardel_procs; p; p = p->next) if (p->id == id) return p;
    return NULL;
}
/* Under the lock. */
static void gardel_proc_release(struct gardel_proc* p) {
    if (--p->refs > 0) return;
    gardel_os_close(p->in_w);
    gardel_os_free(p);
    gardel_cond_free(&p->in_cv);
    free(p->in);
    free(p);
}

static void gardel_proc_done(struct gardel_proc* p) {
    gardel_lock();
    if (--p->running == 0) {
        struct gardel_answer* a = gardel_answer_new(p->attached ? GARDEL_HOST_ATTACHED_ENDED : GARDEL_HOST_PROC_EXITED);
        a->ev.id = p->id;
        a->ev.tag = p->tag;
        a->ev.code = p->code;
        a->ev.ok = true;
        gardel_answer_push(a);
    }
    gardel_proc_release(p);
    gardel_unlock();
}

struct gardel_pipe_job { struct gardel_proc* p; bool err; };
static void gardel_proc_reader(void* arg) {
    struct gardel_pipe_job j = *(struct gardel_pipe_job*)arg;
    free(arg);
    struct gardel_proc* p = j.p;
#ifdef _WIN32
    void* fd = j.err ? p->err_r : p->out_r;
#else
    int fd = j.err ? p->err_r : p->out_r;
#endif
    char* buf = (char*)malloc(GARDEL_PROC_READ);
    if (!buf) abort();
    for (;;) {
        long got = gardel_os_pipe_read(fd, buf, GARDEL_PROC_READ);
        if (got <= 0) break;
        struct gardel_answer* a = gardel_answer_new(j.err ? GARDEL_HOST_PROC_ERR : GARDEL_HOST_PROC_OUT);
        a->ev.id = p->id;
        a->ev.buf = (char*)malloc((size_t)got);
        if (!a->ev.buf) abort();
        memcpy(a->ev.buf, buf, (size_t)got);
        a->ev.buf_len = (size_t)got;
        gardel_lock();
        while (gardel_proc_queued >= GARDEL_PROC_QUEUE_MAX || gardel_pending_answers >= GARDEL_ANSWERS_MAX) {
            gardel_cond_wait(&gardel_space_cv, UINT64_MAX);
        }
        gardel_proc_queued += (uint64_t)got;
        gardel_answer_push(a);
        gardel_unlock();
    }
    free(buf);
    gardel_os_close(fd);
    gardel_proc_done(p);
}

static void gardel_proc_waiter(void* arg) {
    struct gardel_proc* p = (struct gardel_proc*)arg;
    int64_t code = gardel_os_wait(p);
    gardel_lock();
    p->exited = true;
    p->code = code;
    gardel_unlock();
    gardel_proc_done(p);
}

/* Writes what WriteProcess queued, in order. A child that will read no more drops the rest. */
static void gardel_proc_writer(void* arg) {
    struct gardel_proc* p = (struct gardel_proc*)arg;
    gardel_lock();
    for (;;) {
        while (p->in_len == 0 && !p->in_closing) gardel_cond_wait(&p->in_cv, UINT64_MAX);
        if (p->in_len == 0) break;
        char* data = p->in;
        size_t n = p->in_len;
        p->in = NULL;
        p->in_len = p->in_cap = 0;
        bool broken = p->in_broken;
        gardel_unlock();
        if (!broken && !gardel_os_pipe_write(p->in_w, data, n)) broken = true;
        free(data);
        gardel_lock();
        if (broken) p->in_broken = true;
    }
    gardel_os_close(p->in_w);
    p->in_w = GARDEL_PROC_NO_PIPE;
    p->in_writing = false;
    gardel_proc_release(p);
    gardel_unlock();
}

/* Under the lock: no more input. The child sees its input end once queued bytes are written. */
static void gardel_proc_close_input(struct gardel_proc* p) {
    if (p->in_closing) return;
    p->in_closing = true;
    if (p->in_writing) {
        gardel_cond_wake(&p->in_cv);
    } else {
        gardel_os_close(p->in_w);
        p->in_w = GARDEL_PROC_NO_PIPE;
    }
}

/* The loop takes the end of `id`: the table lets it go, and no event names it after this. */
static void gardel_proc_forget(uint64_t id) {
    struct gardel_proc** at = &gardel_procs;
    while (*at && (*at)->id != id) at = &(*at)->next;
    struct gardel_proc* p = *at;
    if (!p) return;
    *at = p->next;
    gardel_lock();
    gardel_proc_close_input(p);
    gardel_proc_release(p);
    gardel_unlock();
}

static void gardel_attach_end(void);
static bool gardel_attach_begin(void);

/* GardelString arguments, borrowed from the command. */
static void gardel_host_run(uint64_t tag, GardelString program, const GardelString* args, size_t n, GardelString dir, bool attached) {
    if (gardel_host_script) {
        fprintf(stderr, "proc: %s %llu ", attached ? "RunAttached" : "RunProcess", (unsigned long long)tag);
        gardel_trace_text(program.data, program.len);
        for (size_t i = 0; i < n; i++) {
            fprintf(stderr, " ");
            gardel_trace_text(args[i].data, args[i].len);
        }
        fprintf(stderr, " in ");
        gardel_trace_text(dir.data, dir.len);
        fprintf(stderr, "\n");
        return;
    }
    struct gardel_proc* p = (struct gardel_proc*)calloc(1, sizeof *p);
    if (!p) abort();
    p->id = gardel_next_proc++;
    p->tag = tag;
    p->attached = attached;
    gardel_cond_init(&p->in_cv);
    int64_t err = 0;
    if (attached && !gardel_attach_begin()) err = GARDEL_FILE_ERR_BUSY;
    if (!err) {
        const char** a = (const char**)malloc(sizeof(char*) * (n + 1));
        size_t* l = (size_t*)malloc(sizeof(size_t) * (n + 1));
        if (!a || !l) abort();
        for (size_t i = 0; i < n; i++) { a[i] = args[i].data; l[i] = (size_t)args[i].len; }
#ifndef _WIN32
        /* A child that has gone must be an error on a write, not the end of this process. */
        signal(SIGPIPE, SIG_IGN);
#endif
        fflush(stdout);
        err = gardel_os_spawn(p, program.data, (size_t)program.len, a, l, n, dir.data, (size_t)dir.len);
        free(a);
        free(l);
        if (err && attached) gardel_attach_end();
    }
    if (err) {
        gardel_cond_free(&p->in_cv);
        free(p);
        struct gardel_answer* a = gardel_answer_new(attached ? GARDEL_HOST_ATTACHED_ENDED : GARDEL_HOST_PROC_FAILED);
        a->ev.tag = tag;
        a->ev.err = err;
        a->ev.ok = false;
        gardel_answer_post(a);
        return;
    }
    p->next = gardel_procs;
    gardel_procs = p;
    /* The table's reference, and one per thread. */
    p->refs = 1;
    if (!attached) {
        struct gardel_answer* a = gardel_answer_new(GARDEL_HOST_PROC_STARTED);
        a->ev.tag = tag;
        a->ev.id = p->id;
        gardel_answer_post(a);
        p->running = 3;
        p->refs += 3;
        struct gardel_pipe_job* o = (struct gardel_pipe_job*)malloc(sizeof *o);
        struct gardel_pipe_job* e = (struct gardel_pipe_job*)malloc(sizeof *e);
        if (!o || !e) abort();
        *o = (struct gardel_pipe_job){ p, false };
        *e = (struct gardel_pipe_job){ p, true };
        gardel_thread_run(gardel_proc_reader, o);
        gardel_thread_run(gardel_proc_reader, e);
    } else {
        p->in_closing = true;
        p->running = 1;
        p->refs += 1;
    }
    gardel_thread_run(gardel_proc_waiter, p);
}

static void gardel_host_proc_write(uint64_t id, GardelBytes data) {
    if (gardel_host_script) {
        fprintf(stderr, "proc: WriteProcess %llu ", (unsigned long long)id);
        gardel_trace_bytes(data);
        fprintf(stderr, "\n");
        return;
    }
    struct gardel_proc* p = gardel_proc_find(id);
    if (!p || p->attached || data.len == 0) return;
    gardel_lock();
    if (!p->in_closing && !p->in_broken) {
        p->in = (char*)gardel_grow(p->in, &p->in_cap, p->in_len + (size_t)data.len, 1);
        memcpy(p->in + p->in_len, data.data, (size_t)data.len);
        p->in_len += (size_t)data.len;
        if (!p->in_writing) {
            p->in_writing = true;
            p->refs++;
            gardel_thread_run(gardel_proc_writer, p);
        }
        gardel_cond_wake(&p->in_cv);
    }
    gardel_unlock();
}

static void gardel_host_proc_close_input(uint64_t id) {
    if (gardel_host_script) {
        fprintf(stderr, "proc: CloseInput %llu\n", (unsigned long long)id);
        return;
    }
    struct gardel_proc* p = gardel_proc_find(id);
    if (!p || p->attached) return;
    gardel_lock();
    gardel_proc_close_input(p);
    gardel_unlock();
}

static void gardel_host_proc_stop(uint64_t id) {
    if (gardel_host_script) {
        fprintf(stderr, "proc: StopProcess %llu\n", (unsigned long long)id);
        return;
    }
    struct gardel_proc* p = gardel_proc_find(id);
    if (!p || p->attached) return;
    gardel_lock();
    gardel_os_stop(p);
    gardel_unlock();
}

/* After Stop: no child outlives the program. */
static void gardel_proc_finish(void) {
    gardel_lock();
    for (struct gardel_proc* p = gardel_procs; p; p = p->next) gardel_os_stop(p);
    gardel_unlock();
}

/* RunAttached: the child gets the terminal as the program found it. The reader is parked first,
   so that it takes none of the child's keys, and a line read waiting is a refusal: its read
   cannot be called back. */
static bool gardel_attach_begin(void) {
    gardel_lock();
    if (gardel_attached || gardel_lines_owed > 0) {
        gardel_unlock();
        return false;
    }
    gardel_attached = true;
    gardel_attached_now = 1;
    gardel_reader_wake_now();
    uint64_t until = gardel_host_now_ms() + 1000;
    while (gardel_reader_in_gate && !gardel_reader_parked) {
        uint64_t now = gardel_host_now_ms();
        if (now >= until) break;
        gardel_cond_wait(&gardel_park_cv, until - now);
    }
    gardel_unlock();
    fflush(stdout);
    gardel_term_set_raw(false);
    gardel_term_out_restore();
    return true;
}

/* As the loop takes AttachedEnded: the terminal is the program's again, and its size is owed,
   as the child may have changed it. */
static void gardel_attach_end(void) {
    gardel_term_out_open();
    gardel_lock();
    gardel_attached = false;
    gardel_attached_now = 0;
    gardel_cond_wake(&gardel_park_cv);
    gardel_unlock();
    if (gardel_size_watching) gardel_size_owed = true;
}

static void gardel_answer_taken(struct gardel_host_event* ev) {
    switch (ev->kind) {
    case GARDEL_HOST_FILE_READ:
    case GARDEL_HOST_PROC_OUT:
    case GARDEL_HOST_PROC_ERR:
    case GARDEL_HOST_PASTED:
        if (ev->buf) {
            ev->data = gardel_bytes_copy(ev->buf, ev->buf_len);
            free(ev->buf);
            ev->buf = NULL;
        }
        break;
    case GARDEL_HOST_PROC_EXITED:
        gardel_proc_forget(ev->id);
        break;
    case GARDEL_HOST_ATTACHED_ENDED:
        if (ev->ok) {
            gardel_attach_end();
            gardel_proc_forget(ev->id);
        }
        break;
    default:
        break;
    }
}

/* Log lines of these events. */
static bool gardel_log_host_event(struct gardel_host_event* ev) {
    switch (ev->kind) {
    case GARDEL_HOST_PASTED:
        gardel_log_word("Pasted:");
        gardel_log_text((const char*)ev->data.data, ev->data.len);
        return true;
    case GARDEL_HOST_FILE_READ:
    case GARDEL_HOST_DIR_LISTED:
    case GARDEL_HOST_ATTACHED_ENDED:
    case GARDEL_HOST_FILE_WRITTEN: {
        bool file = ev->kind == GARDEL_HOST_FILE_READ, dir = ev->kind == GARDEL_HOST_DIR_LISTED;
        bool written = ev->kind == GARDEL_HOST_FILE_WRITTEN;
        if (ev->ok) gardel_log_word(file ? "FileRead:" : dir ? "DirListed:" : written ? "FileWritten:" : "AttachedEnded:");
        else gardel_log_word(file ? "FileReadFailed:" : dir ? "DirListFailed:" : written ? "WriteFailed:" : "AttachFailed:");
        gardel_log_number(ev->tag);
        gardel_log_word(":");
        if (!ev->ok) {
            gardel_log_number((uint64_t)ev->err);
        } else if (written) {
            gardel_log_number(ev->id);
        } else if (file) {
            gardel_log_text(ev->data.data, ev->data.len);
        } else if (dir) {
            /* Each entry is `d` or `f`, its name, and a `/`, which no name holds. */
            for (size_t i = 0; i < ev->item_count; i++) {
                gardel_log_text(ev->items[i].is_dir ? "d" : "f", 1);
                gardel_log_text(ev->items[i].name, ev->items[i].len);
                gardel_log_text("/", 1);
            }
        } else {
            gardel_log_number((uint64_t)ev->code);
        }
        return true;
    }
    case GARDEL_HOST_PROC_STARTED:
    case GARDEL_HOST_PROC_FAILED:
        gardel_log_word(ev->kind == GARDEL_HOST_PROC_STARTED ? "ProcessStarted:" : "ProcessFailed:");
        gardel_log_number(ev->tag);
        gardel_log_word(":");
        gardel_log_number(ev->kind == GARDEL_HOST_PROC_STARTED ? ev->id : (uint64_t)ev->err);
        return true;
    case GARDEL_HOST_PROC_OUT:
    case GARDEL_HOST_PROC_ERR:
        gardel_log_word(ev->kind == GARDEL_HOST_PROC_OUT ? "ProcessOutput:" : "ProcessErrorOutput:");
        gardel_log_number(ev->id);
        gardel_log_word(":");
        gardel_log_text(ev->data.data, ev->data.len);
        return true;
    case GARDEL_HOST_PROC_EXITED:
        gardel_log_word("ProcessExited:");
        gardel_log_number(ev->id);
        gardel_log_word(":");
        gardel_log_number((uint64_t)ev->code);
        return true;
    default:
        return false;
    }
}

/* Script tokens of these events, as the log writes them. */
static bool gardel_script_host(const char* name, size_t n, struct gardel_host_event* ev) {
    static const struct { const char* name; enum gardel_host_kind kind; bool ok; } kinds[] = {
        {"FileRead:", GARDEL_HOST_FILE_READ, true}, {"FileReadFailed:", GARDEL_HOST_FILE_READ, false},
        {"DirListed:", GARDEL_HOST_DIR_LISTED, true}, {"DirListFailed:", GARDEL_HOST_DIR_LISTED, false},
        {"AttachedEnded:", GARDEL_HOST_ATTACHED_ENDED, true}, {"AttachFailed:", GARDEL_HOST_ATTACHED_ENDED, false},
        {"ProcessStarted:", GARDEL_HOST_PROC_STARTED, true}, {"ProcessFailed:", GARDEL_HOST_PROC_FAILED, false},
        {"ProcessOutput:", GARDEL_HOST_PROC_OUT, true}, {"ProcessErrorOutput:", GARDEL_HOST_PROC_ERR, true},
        {"ProcessExited:", GARDEL_HOST_PROC_EXITED, true},
        {"FileWritten:", GARDEL_HOST_FILE_WRITTEN, true}, {"WriteFailed:", GARDEL_HOST_FILE_WRITTEN, false},
    };
    /* Pasted:<text>, cleaned as a terminal's paste is. */
    if (n >= 7 && memcmp(name, "Pasted:", 7) == 0) {
        size_t len;
        char* t = gardel_script_text(name, n, 7, &len);
        if (!t) return false;
        size_t clean_len;
        char* clean = gardel_paste_clean(t, len, &clean_len);
        free(t);
        ev->data = gardel_bytes_copy(clean, clean_len);
        free(clean);
        ev->kind = GARDEL_HOST_PASTED;
        ev->ok = true;
        return true;
    }
    for (size_t k = 0; k < sizeof kinds / sizeof kinds[0]; k++) {
        size_t i = strlen(kinds[k].name);
        if (n <= i || memcmp(name, kinds[k].name, i) != 0) continue;
        enum gardel_host_kind kind = kinds[k].kind;
        uint64_t first, v;
        if (!gardel_script_number(name, n, &i, &first) || i == n || name[i] != ':') return false;
        i++;
        bool text = kinds[k].ok && (kind == GARDEL_HOST_FILE_READ || kind == GARDEL_HOST_DIR_LISTED
                                    || kind == GARDEL_HOST_PROC_OUT || kind == GARDEL_HOST_PROC_ERR);
        if (text) {
            size_t len;
            char* t = gardel_script_text(name, n, i, &len);
            if (!t) return false;
            if (kind == GARDEL_HOST_DIR_LISTED) {
                size_t cap = 0, at = 0;
                ev->items = NULL;
                ev->item_count = 0;
                while (at < len) {
                    char* end = (char*)memchr(t + at, '/', len - at);
                    if (!end || (t[at] != 'd' && t[at] != 'f') || end == t + at + 1) {
                        gardel_dir_free(ev);
                        free(t);
                        return false;
                    }
                    gardel_dir_add(&ev->items, &ev->item_count, &cap, t + at + 1, (size_t)(end - t - at - 1), t[at] == 'd');
                    at = (size_t)(end - t) + 1;
                }
            } else {
                ev->data = gardel_bytes_copy(t, len);
            }
            free(t);
        } else {
            if (!gardel_script_number(name, n, &i, &v) || i != n) return false;
            if (kind == GARDEL_HOST_PROC_STARTED || (kind == GARDEL_HOST_FILE_WRITTEN && kinds[k].ok)) ev->id = v;
            else if (!kinds[k].ok) ev->err = (int64_t)v;
            else {
                if (v > (uint64_t)INT64_MAX) return false;
                ev->code = (int64_t)v;
            }
        }
        ev->kind = kind;
        ev->ok = kinds[k].ok;
        if (kind == GARDEL_HOST_PROC_OUT || kind == GARDEL_HOST_PROC_ERR || kind == GARDEL_HOST_PROC_EXITED) ev->id = first;
        else ev->tag = first;
        return true;
    }
    return false;
}
"#;

/// The part that names instance types: the values of `FileRead`, `DirListed`,
/// `AttachedEnded`, `FileWritten` and `Pasted`, and the arguments of `RunProcess` and
/// `RunAttached`. `names` gives the C names of `RESULT[IO_ERROR, BYTES]`,
/// `RESULT[IO_ERROR, LIST[DIR_ENTRY]]`, `RESULT[IO_ERROR, I64]`, `LIST[DIR_ENTRY]`,
/// `LIST[STRING]` and `RESULT[IO_ERROR, NAT]` (whose C name is that of `U64`).
pub(crate) fn typed_runtime(names: &[String; 6]) -> String {
    let [bytes_result, dir_result, code_result, dir_list, strings, count_result] = names;
    format!(
        r#"/* The values of file and process events, built by the loop. */
static struct {count_result}* gardel_written_value(struct gardel_host_event ev) {{
    if (!ev.ok) return make_{count_result}_Error(make_IO_ERROR_OsError(ev.err));
    return make_{count_result}_Ok(ev.id);
}}
/* A paste's text, which takes over the bytes: cleaned UTF-8 when it was read, so it is always
   a STRING, and empty should it not be. */
static GardelString gardel_paste_value(struct gardel_host_event ev) {{
    uint64_t chars = 0;
    uint64_t n = ev.data.len;
    struct STRING_ERROR* bad = gardel_utf8_validate(ev.data.data, n, &chars);
    if (bad) {{
        drop_STRING_ERROR(bad);
        n = 0;
        chars = 0;
    }}
    GardelString s = {{ (const char*)ev.data.data, n, chars, ev.data.owner }};
    return s;
}}
static struct {bytes_result}* gardel_file_value(struct gardel_host_event ev) {{
    if (!ev.ok) return make_{bytes_result}_Error(make_IO_ERROR_OsError(ev.err));
    return make_{bytes_result}_Ok(ev.data);
}}
/* A name that is not UTF-8 has no STRING, so it is left out, as the same rule gives on replay. */
static struct {dir_result}* gardel_dir_value(struct gardel_host_event ev) {{
    if (!ev.ok) return make_{dir_result}_Error(make_IO_ERROR_OsError(ev.err));
    struct {dir_list}* xs = NULL;
    for (size_t i = ev.item_count; i-- > 0;) {{
        uint64_t chars = 0;
        struct STRING_ERROR* bad = gardel_utf8_validate((const uint8_t*)ev.items[i].name, (uint64_t)ev.items[i].len, &chars);
        if (bad) {{
            drop_STRING_ERROR(bad);
            continue;
        }}
        void* owner = NULL;
        char* data = gardel_buf_alloc(ev.items[i].len + 1, &owner);
        memcpy(data, ev.items[i].name, ev.items[i].len);
        data[ev.items[i].len] = '\0';
        GardelString s = {{ data, (uint64_t)ev.items[i].len, chars, owner }};
        struct {dir_list}* c = (struct {dir_list}*)gardel_alloc(sizeof(struct {dir_list}));
        atomic_init(&c->_rc, 1);
        c->len = xs ? xs->len + 1 : 1;
        c->head = make_DIR_ENTRY(s, ev.items[i].is_dir);
        c->tail = xs;
        xs = c;
    }}
    gardel_dir_free(&ev);
    return make_{dir_result}_Ok(xs);
}}
static struct {code_result}* gardel_attached_value(struct gardel_host_event ev) {{
    if (!ev.ok) return make_{code_result}_Error(make_IO_ERROR_OsError(ev.err));
    return make_{code_result}_Ok(ev.code);
}}
/* A command's argument list, borrowed: the array is the caller's to free. */
static GardelString* gardel_strings_of(struct {strings}* xs, size_t* n) {{
    *n = xs ? (size_t)xs->len : 0;
    GardelString* a = (GardelString*)malloc(sizeof(GardelString) * (*n + 1));
    if (!a) abort();
    for (size_t i = 0; xs; xs = xs->tail) a[i++] = xs->head;
    return a;
}}
static void gardel_host_run_list(uint64_t tag, GardelString program, struct {strings}* args, GardelString dir, bool attached) {{
    size_t n;
    GardelString* a = gardel_strings_of(args, &n);
    gardel_host_run(tag, program, a, n, dir, attached);
    free(a);
}}
/* `<tag>:<program>:<dir>`, then `:<arg>` for each. */
static void gardel_log_run(uint64_t tag, GardelString program, struct {strings}* args, GardelString dir) {{
    gardel_log_number(tag);
    gardel_log_word(":");
    gardel_log_text(program.data, program.len);
    gardel_log_word(":");
    gardel_log_text(dir.data, dir.len);
    for (; args; args = args->tail) {{
        gardel_log_word(":");
        gardel_log_text(args->head.data, args->head.len);
    }}
}}

"#
    )
}
