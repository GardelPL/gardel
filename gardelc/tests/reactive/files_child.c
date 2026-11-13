/* The child that the programs of tests/files_procs.rs run. `orphan` ends at once, leaving a
   child that writes `late` half a second later. `sleep`
   runs until stopped, `attached` writes a line and ends with 3, `readline` echoes a line of its
   input, `flood N` writes N bytes; otherwise it echoes its arguments, its directory and a line
   of its input, writes to stderr and ends with 7. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifdef _WIN32
#include <direct.h>
#include <windows.h>
#define getcwd _getcwd
#else
#include <unistd.h>
#endif

int main(int argc, char** argv) {
    if (argc > 1 && strcmp(argv[1], "sleep") == 0) {
        for (;;) {
#ifdef _WIN32
            Sleep(1000);
#else
            sleep(1);
#endif
        }
    }
    if (argc > 1 && strcmp(argv[1], "attached") == 0) {
        printf("attached child\n");
        return 3;
    }
    if (argc > 1 && strcmp(argv[1], "readline") == 0) {
        char line[256];
        if (!fgets(line, sizeof line, stdin)) line[0] = 0;
        line[strcspn(line, "\r\n")] = 0;
        printf("child read [%s]\n", line);
        return 0;
    }
    if (argc > 1 && strcmp(argv[1], "late") == 0) {
#ifdef _WIN32
        Sleep(500);
#else
        usleep(500000);
#endif
        printf("late\n");
        return 0;
    }
    if (argc > 1 && strcmp(argv[1], "orphan") == 0) {
        /* Ends at once, leaving a child of its own that writes to the same output later. */
#ifdef _WIN32
        char cmd[4200];
        snprintf(cmd, sizeof cmd, "\"%s\" late", argv[0]);
        STARTUPINFOA si;
        PROCESS_INFORMATION pi;
        memset(&si, 0, sizeof si);
        si.cb = sizeof si;
        si.dwFlags = STARTF_USESTDHANDLES;
        si.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
        si.hStdOutput = GetStdHandle(STD_OUTPUT_HANDLE);
        si.hStdError = GetStdHandle(STD_ERROR_HANDLE);
        if (!CreateProcessA(NULL, cmd, NULL, NULL, TRUE, 0, NULL, NULL, &si, &pi)) return 9;
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);
#else
        if (fork() == 0) {
            execl(argv[0], argv[0], "late", (char*)NULL);
            _exit(9);
        }
#endif
        return 0;
    }
    if (argc > 2 && strcmp(argv[1], "flood") == 0) {
        long n = atol(argv[2]);
        for (long i = 0; i < n; i++) putchar('a' + (int)(i % 26));
        return 0;
    }
    for (int i = 1; i < argc; i++) printf("arg [%s]\n", argv[i]);
    char buf[4096];
    if (getcwd(buf, sizeof buf)) printf("cwd [%s]\n", buf);
    if (fgets(buf, sizeof buf, stdin)) {
        buf[strcspn(buf, "\r\n")] = 0;
        printf("in [%s]\n", buf);
    }
    /* The parent closes the input once it sees this. */
    fflush(stdout);
    if (fgets(buf, sizeof buf, stdin) == NULL) printf("input ended\n");
    fflush(stdout);
    fprintf(stderr, "to stderr\n");
    return 7;
}
