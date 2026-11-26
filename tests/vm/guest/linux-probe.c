/*
 * Asks a kernel which Linux system calls it has.
 *
 * Built without a C library, so that the same file runs under Linuxulator and in a
 * Linux guest. Every call is made with arguments that cannot succeed in changing
 * anything; the only question is whether the kernel knows the call.
 *
 *   linux-probe            print every call and its result
 *   linux-probe NAME       exit 0 if the call exists, 38 (ENOSYS) if not
 *   linux-probe @echo ...  print the arguments, then exit with 7
 */

#define ENOSYS 38

static long
call(long number, long a, long b, long c, long d, long e)
{
	long result;
	register long r10 __asm__("r10") = d;
	register long r8 __asm__("r8") = e;

	__asm__ volatile("syscall" : "=a"(result)
	    : "a"(number), "D"(a), "S"(b), "d"(c), "r"(r10), "r"(r8)
	    : "rcx", "r11", "memory");
	return (result);
}

static const struct {
	const char *name;
	long number, a, b, c, d, e;
} calls[] = {
	/* Controls: both kernels have these. */
	{ "getpid", 39, 0, 0, 0, 0, 0 },
	{ "statx", 332, -1, 0, 0, 0, 0 },
	{ "memfd_create", 319, 0, 0, 0, 0, 0 },
	/* Namespaces. */
	{ "unshare", 272, 0, 0, 0, 0, 0 },
	{ "setns", 308, -1, 0, 0, 0, 0 },
	{ "pivot_root", 155, 0, 0, 0, 0, 0 },
	{ "clone3", 435, 0, 0, 0, 0, 0 },
	/* Kernel modules. */
	{ "init_module", 175, 0, 0, 0, 0, 0 },
	{ "finit_module", 313, -1, 0, 0, 0, 0 },
	/* Newer interfaces. */
	{ "bpf", 321, -1, 0, 0, 0, 0 },
	{ "perf_event_open", 298, 0, 0, 0, -1, 0 },
	{ "userfaultfd", 323, -1, 0, 0, 0, 0 },
	{ "io_uring_setup", 425, 0, 0, 0, 0, 0 },
	{ "fsopen", 430, 0, 0, 0, 0, 0 },
	{ "pidfd_open", 434, -1, 0, 0, 0, 0 },
	{ "openat2", 437, -1, 0, 0, 0, 0 },
	{ "landlock_create_ruleset", 444, 0, 0, 1, 0, 0 },
};

static unsigned long
length(const char *text)
{
	unsigned long n = 0;

	while (text[n] != '\0')
		n++;
	return (n);
}

static void
print(const char *text)
{
	call(1, 1, (long)text, (long)length(text), 0, 0);
}

static int
same(const char *a, const char *b)
{
	while (*a != '\0' && *a == *b)
		a++, b++;
	return (*a == *b);
}

__attribute__((noreturn)) static void
quit(int code)
{
	call(231, code, 0, 0, 0, 0);
	for (;;)
		;
}

__attribute__((used)) static void
run(long *stack)
{
	long argc = stack[0];
	char **argv = (char **)(stack + 1);
	unsigned int i;

	if (argc > 1 && same(argv[1], "@echo")) {
		for (i = 2; i < argc; i++) {
			print("[");
			print(argv[i]);
			print("]\n");
		}
		quit(7);
	}
	for (i = 0; i < sizeof(calls) / sizeof(calls[0]); i++) {
		long result;

		if (argc > 1 && !same(argv[1], calls[i].name))
			continue;
		result = call(calls[i].number, calls[i].a, calls[i].b,
		    calls[i].c, calls[i].d, calls[i].e);
		if (argc > 1)
			quit(result == -ENOSYS ? ENOSYS : 0);
		print(calls[i].name);
		print(result == -ENOSYS ? " missing\n" : " present\n");
	}
	quit(argc > 1 ? 1 : 0);
}

__asm__(".globl _start\n_start:\n\tmov %rsp, %rdi\n\tand $-16, %rsp\n\tcall run\n");
