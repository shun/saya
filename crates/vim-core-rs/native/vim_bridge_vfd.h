#ifndef VIM_BRIDGE_VFD_H
#define VIM_BRIDGE_VFD_H

#include <stddef.h>
#include <sys/types.h>
#include <poll.h>
#include <unistd.h>
#include <stdbool.h>

#define VFD_MIN_FD 512

// These are exported by Rust (or implemented in Rust)
ssize_t vim_core_vfd_read(int fd, void *buf, size_t count);
ssize_t vim_core_vfd_write(int fd, const void *buf, size_t count);
int vim_core_vfd_close(int fd);
int vim_core_vfd_poll(struct pollfd *fds, nfds_t nfds, int timeout);

// These are used as replacements for the standard C library functions in Vim source code.
ssize_t vim_bridge_vfd_read(int fd, void *buf, size_t count);
ssize_t vim_bridge_vfd_write(int fd, const void *buf, size_t count);
int vim_bridge_vfd_close(int fd);
#include <sys/select.h>

int vim_bridge_vfd_poll(struct pollfd *fds, nfds_t nfds, int timeout);
int vim_bridge_vfd_select(int nfds, fd_set *readfds, fd_set *writefds, fd_set *exceptfds, struct timeval *timeout);

#endif
