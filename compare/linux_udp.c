#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <arpa/inet.h>
#include <netinet/ip.h>
#include <sys/epoll.h>
#include <sys/types.h>
#include <sys/socket.h>

#define MAX_EVENTS (128)
#define BUF_LEN (65535)

int init_epoll(int sock_a, int sock_b);
int init_udp_sock(const char* bind_addr);
void register_epoll(int epoll_fd, int read_fd);
void forward_messages(int from, int to);

int main(int argc, char** argv) {
	struct epoll_event events[MAX_EVENTS];
	int sock_a, sock_b, fd_epoll, nfds;

	if(argc != 3) {
		printf("Usage: linux_udp <bind_addr_1> <bind_addr_2>\n");
		exit(1);
	}

	sock_a = init_udp_sock(argv[1]);
	sock_b = init_udp_sock(argv[2]);
	fd_epoll = init_epoll(sock_a, sock_b);

	for(;;) {
		nfds = epoll_wait(fd_epoll, events, MAX_EVENTS, -1);
		if(nfds < 0) {
			perror("Failed to epoll_wait");
			exit(1);
		}

		for(int i = 0; i < nfds; i++) {
			if(events[i].data.fd == sock_a) {
				forward_messages(sock_a, sock_b);
			} else if(events[i].data.fd == sock_b) {
				forward_messages(sock_b, sock_a);
			}
		}
	}
}

int init_epoll(int sock_a, int sock_b) {
	int fd_epoll;

	fd_epoll = epoll_create1(0);
	if(fd_epoll < 0) {
		perror("Failed to create epoll");
		exit(1);
	}

	register_epoll(fd_epoll, sock_a);
	register_epoll(fd_epoll, sock_b);

	return fd_epoll;
}

int init_udp_sock(const char* bind_addr) {
	struct sockaddr_in addr;
	int sock;

	sock = socket(AF_INET, SOCK_DGRAM | SOCK_NONBLOCK, 0);
	if(sock < 0) {
		perror("Failed to create socket");
		exit(1);
	}

	addr.sin_family = AF_INET;
	addr.sin_port = 243;
	if(inet_aton(bind_addr, &addr.sin_addr) == 0) {
		perror("Not a valid address");
		exit(1);
	}

	if(bind(sock, (struct sockaddr *) &addr, sizeof(struct sockaddr_in)) < 0) {
		perror("Failed to bind socket to address");
		exit(1);
	}

	return sock;
}

void register_epoll(int fd_epoll, int fd_read) {
	struct epoll_event ev;

	ev.events = EPOLLIN;
	ev.data.fd = fd_read;

	if(epoll_ctl(fd_epoll, EPOLL_CTL_ADD, fd_read, &ev) < 0) {
		perror("Failed to register socket for read");
		exit(1);
	}
}

void forward_messages(int from, int to) {
	static char BUF[BUF_LEN];

	struct sockaddr_in addr;
	socklen_t addr_len;
	ssize_t len;

	for(;;) {
		addr_len = sizeof(struct sockaddr_in);
		len = recvfrom(from, BUF, BUF_LEN, 0,
			(struct sockaddr *) &addr, &addr_len);
		if(len < 0) {
			if(errno == EAGAIN || errno == EWOULDBLOCK)
				break;
			perror("Failed recv with unknown error");
			exit(1);
		}

		// We do not care about success. Failed messages are simply dropped.
		sendto(to, BUF, len, 0,
			(struct sockaddr *) &addr, addr_len);
	}
}
