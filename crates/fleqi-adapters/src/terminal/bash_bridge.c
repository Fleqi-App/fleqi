/* Fleqi's session-local GNU Bash loadable builtin (AGPL-3.0-only).
 * The small loadable ABI below is shared by Bash 5.1, 5.2.11+, and 5.3.
 * Do not link libreadline: these symbols must refer to Bash's own editor. */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <poll.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

typedef struct word_desc { char *word; int flags; } WORD_DESC;
typedef struct word_list { struct word_list *next; WORD_DESC *word; } WORD_LIST;
struct builtin {
  char *name;
  int (*function)(WORD_LIST *);
  int flags;
  char *const *doc;
  const char *usage;
  char *handle;
};

extern int rl_end, rl_done, executing, executing_builtin, last_command_exit_value;
extern int rl_readline_version;
extern unsigned long rl_readline_state;
extern char *ps1_prompt, *ps2_prompt, *current_prompt_string;
extern int (*rl_getc_function)(FILE *);
extern int (*rl_event_hook)(void), (*rl_signal_event_hook)(void);
extern int rl_getc(FILE *), rl_set_prompt(const char *), rl_forced_update_display(void);
extern void rl_check_signals(void);
extern int cd_builtin(WORD_LIST *);
extern char *get_string_value(const char *);

#define FRAME_LIMIT 65536
#define PATH_LIMIT 16384
static int control_fd = -1, visible, minor_version;
static pid_t owner_pid;
static int (*original_getc)(FILE *);
static void *decode_prompt;
static int (*timeout_remaining)(unsigned int *, unsigned int *);
static uint64_t generation, pending_generation, pending_revision, input_generation;
static char pending_path[PATH_LIMIT];
static char input[FRAME_LIMIT + 1];
static size_t input_length;
static int last_ready = -1, last_edit = -1;
static char last_cwd[PATH_LIMIT];

static int get_input(FILE *stream);

static int user_input(FILE *stream) {
  /* A fast command can finish without another idle callback in between. */
  last_ready = -1;
  return original_getc(stream);
}

static void disconnect_control(void) {
  if (control_fd >= 0) close(control_fd);
  control_fd = -1;
  pending_path[0] = 0;
  if (rl_getc_function == get_input) rl_getc_function = original_getc;
}

/* Stream writes are bounded and nonblocking. A partial frame fails closed. */
static int send_frame(const char *frame, size_t length) {
  ssize_t sent = send(control_fd, frame, length, MSG_NOSIGNAL | MSG_DONTWAIT);
  if (sent != (ssize_t)length) { disconnect_control(); return 0; }
  return 1;
}

static int hex_path(char *output, const char *path) {
  static const char digits[] = "0123456789abcdef";
  size_t length = strlen(path);
  if (!length || length >= PATH_LIMIT) return 0;
  for (size_t i = 0; i < length; i++) {
    unsigned char byte = (unsigned char)path[i];
    output[2 * i] = digits[byte >> 4];
    output[2 * i + 1] = digits[byte & 15];
  }
  output[2 * length] = 0;
  return 1;
}

static int from_hex(char byte) {
  if (byte >= '0' && byte <= '9') return byte - '0';
  if (byte >= 'a' && byte <= 'f') return byte - 'a' + 10;
  return -1;
}

static int decode_path(char *output, const char *text) {
  size_t length = strlen(text);
  if (!length || length % 2 || length / 2 >= PATH_LIMIT) return 0;
  for (size_t i = 0; i < length; i += 2) {
    int high = from_hex(text[i]), low = from_hex(text[i + 1]);
    if (high < 0 || low < 0 || !(high | low)) return 0;
    output[i / 2] = (char)((high << 4) | low);
  }
  output[length / 2] = 0;
  return output[0] == '/';
}

static int number(char **cursor, uint64_t *result) {
  char *start = *cursor, *end;
  if (*start < '0' || *start > '9') return 0;
  errno = 0;
  unsigned long long parsed = strtoull(start, &end, 10);
  if (errno || (*end && *end != ' ')) return 0;
  *cursor = *end ? end + 1 : end;
  *result = (uint64_t)parsed;
  return 1;
}

/* D: request serial, context revision, outcome (0 success/1 failure/2 cancelled), cwd. */
static void result(int outcome) {
  char *cwd = getcwd(NULL, 0), encoded[PATH_LIMIT * 2], frame[FRAME_LIMIT];
  if (!cwd || !hex_path(encoded, cwd)) { free(cwd); disconnect_control(); return; }
  free(cwd);
  int length = snprintf(frame, sizeof(frame), "D %" PRIu64 " %" PRIu64 " %d %s\n",
             pending_generation, pending_revision, outcome, encoded);
  pending_path[0] = 0;
  last_ready = -1;
  if (length <= 0 || (size_t)length >= sizeof(frame)) { disconnect_control(); return; }
  send_frame(frame, (size_t)length);
}

static int command(char *frame) {
  char kind = frame[0], *cursor = frame + 2;
  uint64_t serial, revision, setting;
  char path[PATH_LIMIT];
  if (frame[1] != ' ' || !number(&cursor, &serial) || serial <= generation) return 0;
  if (kind == 'C') {
    if (!number(&cursor, &revision) || !decode_path(path, cursor)) return 0;
  } else if (kind == 'V') {
    if (!number(&cursor, &setting) || *cursor || setting > 1) return 0;
  } else if ((kind != 'X' && kind != 'I') || *cursor) return 0;
  generation = serial;
  if (kind == 'I') { input_generation = serial; last_ready = -1; return 1; }
  if (pending_path[0]) result(2);
  if (control_fd < 0) return 0;
  if (kind == 'C') {
    pending_generation = serial;
    pending_revision = revision;
    strcpy(pending_path, path);
  } else if (kind == 'V') visible = (int)setting;
  last_ready = -1;
  return 1;
}

static void receive_commands(void) {
  for (;;) {
    ssize_t received = recv(control_fd, input + input_length, FRAME_LIMIT - input_length, MSG_DONTWAIT);
    if (received < 0 && (errno == EAGAIN || errno == EWOULDBLOCK)) return;
    if (received < 0 && errno == EINTR) continue;
    if (received <= 0) { disconnect_control(); return; }
    input_length += (size_t)received;
    size_t consumed = 0;
    for (size_t i = 0; i < input_length; i++) {
      if (!input[i]) { disconnect_control(); return; }
      if (input[i] != '\n') continue;
      input[i] = 0;
      if (i - consumed < 3 || !command(input + consumed)) { disconnect_control(); return; }
      consumed = i + 1;
    }
    memmove(input, input + consumed, input_length - consumed);
    input_length -= consumed;
    if (input_length == FRAME_LIMIT) { disconnect_control(); return; }
  }
}

static int safe_prompt(void) {
  /* INITIALIZED, TERMPREPPED, READCMD, TTYCSAVED, VICMDONCE only.
   * Excludes search, macros, completion, multibyte input, PS2 and read -e. */
  const unsigned long ordinary = 0x2 | 0x4 | 0x8 | 0x40000 | 0x400000;
  return !executing && !executing_builtin && current_prompt_string &&
      current_prompt_string == ps1_prompt && current_prompt_string != ps2_prompt &&
      !rl_end && !rl_done && (rl_readline_state & 0x8) && !(rl_readline_state & ~ordinary);
}

static void publish_state(int ready) {
  char *cwd = getcwd(NULL, 0), encoded[PATH_LIMIT * 2], frame[FRAME_LIMIT];
  if (!cwd || !hex_path(encoded, cwd)) { free(cwd); disconnect_control(); return; }
  if (ready == last_ready && rl_end == last_edit && strcmp(cwd, last_cwd) == 0) { free(cwd); return; }
  strcpy(last_cwd, cwd);
  free(cwd);
  last_ready = ready;
  last_edit = rl_end;
  int length = snprintf(frame, sizeof(frame), "S %" PRIu64 " %d %d %s\n", input_generation, ready, rl_end < 0 ? 0 : rl_end, encoded);
  if (length <= 0 || (size_t)length >= sizeof(frame)) { disconnect_control(); return; }
  send_frame(frame, (size_t)length);
}

static int change_directory(const char *path) {
  /* Pin the exact directory: user cdspell/CDPATH settings must not turn a
   * stale path into a different directory. -P keeps PWD independent of fd. */
  int directory = open(path, O_PATH | O_DIRECTORY | O_CLOEXEC);
  int status = 1;
  if (directory >= 0) {
    char fd_path[64];
    snprintf(fd_path, sizeof(fd_path), "/proc/self/fd/%d", directory);
    WORD_DESC argument = { fd_path, 0 }, separator = { "--", 0 }, physical = { "-P", 0 };
    WORD_LIST tail = { NULL, &argument }, middle = { &tail, &separator }, head = { &middle, &physical };
    status = cd_builtin(&head);
    close(directory);
  } else {
    fprintf(stderr, "Fleqi: directory change failed: %s\n", strerror(errno));
  }
  return status;
}

static void idle(FILE *stream) {
  receive_commands();
  if (control_fd < 0) return;
  struct pollfd keyboard = { fileno(stream), POLLIN, 0 };
  int ready = visible && safe_prompt() && poll(&keyboard, 1, 0) == 0;
  if (ready && pending_path[0]) {
    int saved_status = last_command_exit_value;
    int status = change_directory(pending_path);
    /* Bash 5.3 added the is_prompt parameter. Keep both supported ABIs. */
    char *raw_prompt = get_string_value("PS1");
    char *prompt = !raw_prompt ? NULL : minor_version >= 3
      ? ((char *(*)(char *, int))decode_prompt)(raw_prompt, 1)
      : ((char *(*)(char *))decode_prompt)(raw_prompt);
    if (prompt) { rl_set_prompt(prompt); free(prompt); rl_forced_update_display(); }
    last_command_exit_value = saved_status;
    /* Prompt expansion may run user commands: acknowledge only after it ends. */
    result(status == 0 ? 0 : 1);
  }
  if (control_fd >= 0) publish_state(ready);
}

static int get_input(FILE *stream) {
  /* Forked subshells must not consume their parent's control channel. */
  if (getpid() != owner_pid || control_fd < 0) return original_getc(stream);
  struct pollfd keyboard = { fileno(stream), POLLIN, 0 };
  for (;;) {
    if (poll(&keyboard, 1, 0) > 0) return user_input(stream);
    idle(stream);
    if (control_fd < 0) return original_getc(stream);
    int timeout_ms = 100;
    if (timeout_remaining) {
      unsigned int seconds, microseconds;
      int timed = timeout_remaining(&seconds, &microseconds);
      if (timed == 0) return original_getc(stream);
      if (timed > 0 && seconds == 0 && microseconds < 100000)
        timeout_ms = (int)((microseconds + 999) / 1000);
    }
    int outcome = poll(&keyboard, 1, timeout_ms);
    if (outcome > 0 || (outcome < 0 && errno != EINTR)) return user_input(stream);
    if (outcome < 0) {
      rl_check_signals();
      if (rl_signal_event_hook) rl_signal_event_hook();
    }
  }
}

int fleqi_sync_builtin(WORD_LIST *args) {
  if (control_fd >= 0 || !args || !args->next || !args->next->next || args->next->next->next) return 1;
  const char *initial_directory = args->next->next->word->word;
  if (initial_directory[0] != '/' || strlen(initial_directory) >= PATH_LIMIT) return 1;
  const char *version = get_string_value("BASH_VERSION");
  if (!version || version[0] != '5' || version[1] != '.' || version[2] < '1' ||
    version[2] > '3' || version[3] != '.' || rl_readline_version < 0x0801 ||
    rl_readline_version > 0x0803 || rl_getc_function != rl_getc || rl_event_hook) return 1;
  minor_version = version[2] - '0';
  if (minor_version == 2) {
    char *end;
    errno = 0;
    unsigned long patch = strtoul(version + 4, &end, 10);
    if (errno || end == version + 4 || patch < 11) {
      fprintf(stderr, "Fleqi: Bash 5.2 needs patch 11 or newer for reliable read timeouts.\n");
      return 1;
    }
  }
  decode_prompt = dlsym(RTLD_DEFAULT, "decode_prompt_string");
  timeout_remaining = (int (*)(unsigned int *, unsigned int *))dlsym(RTLD_DEFAULT, "rl_timeout_remaining");
  if (!decode_prompt) return 1;
  char *pid_text = args->next->word->word;
  uint64_t host_pid;
  if (!number(&pid_text, &host_pid) || *pid_text || !host_pid || host_pid > INT32_MAX) return 1;
  struct sockaddr_un address = { .sun_family = AF_UNIX };
  if (strlen(args->word->word) >= sizeof(address.sun_path)) return 1;
  strcpy(address.sun_path, args->word->word);
  int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
  if (fd < 0) return 1;
  struct ucred peer;
  socklen_t peer_length = sizeof(peer);
  if (connect(fd, (struct sockaddr *)&address, sizeof(address)) != 0 ||
    getsockopt(fd, SOL_SOCKET, SO_PEERCRED, &peer, &peer_length) != 0 ||
    peer.pid != (pid_t)host_pid || peer.uid != getuid() || fcntl(fd, F_SETFL, O_NONBLOCK) < 0) {
    close(fd); return 1;
  }
  /* A signal trap may unload the builtin while get_input is on the stack. */
  Dl_info module;
  if (!dladdr((void *)fleqi_sync_builtin, &module) ||
    !dlopen(module.dli_fname, RTLD_NOW | RTLD_NODELETE)) { close(fd); return 1; }
  if (change_directory(initial_directory) != 0) { close(fd); return 1; }
  owner_pid = getpid();
  control_fd = fd;
  visible = 0;
  generation = 0;
  input_generation = 0;
  input_length = 0;
  pending_path[0] = 0;
  last_cwd[0] = 0;
  last_ready = last_edit = -1;
  original_getc = rl_getc_function;
  if (!send_frame("H 1\n", 4)) return 1;
  rl_getc_function = get_input;
  return 0;
}

void fleqi_sync_builtin_unload(char *name) {
  (void)name;
  disconnect_control();
}

static char *const documentation[] = { "Fleqi private directory synchronization.", NULL };
struct builtin fleqi_sync_struct = {
  "fleqi_sync", fleqi_sync_builtin, 1, documentation, "fleqi_sync socket host-pid directory", NULL
};
