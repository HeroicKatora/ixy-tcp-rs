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

int init_udp_sock(const char* bind_addr);
void init_target_addr(struct sockaddr_in* sock, const char* addr);
void forward_messages(int from, int to, const struct sockaddr_in* via);

int main(int argc, char** argv) {
	int sock_a, sock_b;

	struct sockaddr_in to_a, to_b;

	if(argc != 5) {
		printf("Usage: linux_udp <bind_addr_1> <bind_addr_2>\n");
		exit(1);
	}

	printf("[+] Startup\n");
	sock_a = init_udp_sock(argv[1]);
	sock_b = init_udp_sock(argv[2]);
	init_target_addr(&to_a, argv[3]);
	init_target_addr(&to_b, argv[4]);

	printf("[+] Initialized\n");

	for(;;) {
		forward_messages(sock_a, sock_b, &to_a);
		forward_messages(sock_b, sock_a, &to_b);
	}
}

int init_udp_sock(const char* bind_addr) {
	struct sockaddr_in addr;
	int sock;

	sock = socket(AF_INET, SOCK_DGRAM | SOCK_NONBLOCK, IPPROTO_UDP);
	if(sock < 0) {
		perror("Failed to create socket");
		exit(1);
	}

	addr.sin_family = AF_INET;
	addr.sin_port = htons(319);
	if(inet_aton(bind_addr, &addr.sin_addr) == 0) {
		perror("Not a valid address");
		exit(1);
	}

	printf("[+] Socket created\n");

	if(bind(sock, (struct sockaddr *) &addr, sizeof(struct sockaddr_in)) < 0) {
		perror("Failed to bind socket to address");
		exit(1);
	}

	printf("[+] Socket bound to %s\n", bind_addr);

	return sock;
}

void init_target_addr(struct sockaddr_in* sock, const char* addr) {
	sock->sin_family = AF_INET;
	sock->sin_port = htons(1234);
	if(inet_aton(addr, &sock->sin_addr) == 0) {
		perror("Not a valid address");
		exit(1);
	}
}

void forward_messages(int from, int to, const struct sockaddr_in* via) {
	static char BUF[BUF_LEN];

	struct sockaddr_in addr;
	socklen_t addr_len;
	ssize_t len;

	for(int i = 0;i < 128;i++) {
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
		if(sendto(to, BUF, len, 0,
			(struct sockaddr *) via, sizeof(*via)) < 0) {
			perror("Failed to send");
			// exit(1);
		}
	}
}
