/* fogu.h — C API of the Fogu runtime (libfogu_rt).
 *
 * Build:  cargo build --release -p fogu-ffi
 *         → target/release/libfogu_rt.so | fogu_rt.dll | libfogu_rt.dylib
 *
 * Every function returning `char *` returns a JSON object that the caller
 * frees with fogu_free_string():
 *     {"ok":true,"value":<any JSON>}   or   {"ok":false,"error":"message"}
 * Values cross as JSON. A runtime runs on its own thread: its globals, fg
 * tasks, servers and Fogu.signal handlers stay alive between calls.
 */
#ifndef FOGU_H
#define FOGU_H

#ifdef __cplusplus
extern "C" {
#endif

typedef struct FoguRuntime FoguRuntime;

/* Fogu.host.emit(name, data) in the program calls this (on the runtime's thread).
 * The callback owns `name` and `json`: free them with fogu_free_string(). */
typedef void (*fogu_host_fn)(char *name, char *json, void *user);

const char  *fogu_version(void);                                   /* static, do not free */
FoguRuntime *fogu_new(void);                                       /* NULL on failure */
void         fogu_free(FoguRuntime *rt);                           /* stops tasks and servers */
void         fogu_free_string(char *s);

char *fogu_eval(FoguRuntime *rt, const char *code);                /* value of the last expression */
char *fogu_spawn(FoguRuntime *rt, const char *code);               /* runs as a background task */
char *fogu_run_file(FoguRuntime *rt, const char *path);
char *fogu_call(FoguRuntime *rt, const char *name, const char *args_json);   /* args: JSON array or NULL */
char *fogu_get(FoguRuntime *rt, const char *name);
char *fogu_set(FoguRuntime *rt, const char *name, const char *value_json);
char *fogu_emit(FoguRuntime *rt, const char *name, const char *data_json);  /* Fogu.signal.on handlers */
char *fogu_on_host(FoguRuntime *rt, fogu_host_fn cb, void *user);  /* cb NULL removes it */

#ifdef __cplusplus
}
#endif
#endif
