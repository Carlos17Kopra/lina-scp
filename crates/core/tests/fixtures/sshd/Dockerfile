FROM debian:trixie-slim
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends openssh-server \
 && rm -rf /var/lib/apt/lists/* \
 && mkdir -p /run/sshd /etc/ssh/authorized_keys \
 && sed -i 's/^session\s\+required\s\+pam_loginuid.so/session optional pam_loginuid.so/' /etc/pam.d/sshd \
 && useradd -m -s /bin/bash pwuser && echo 'pwuser:pwpass' | chpasswd \
 && useradd -m -s /bin/bash keyuser && echo 'keyuser:unused-Kq81' | chpasswd \
 && useradd -m -s /bin/bash kbduser && echo 'kbduser:kbdpass' | chpasswd
COPY sshd_config /etc/ssh/sshd_config
COPY keyuser.authorized_keys /etc/ssh/authorized_keys/keyuser
RUN chmod 644 /etc/ssh/authorized_keys/keyuser && sshd -t
EXPOSE 22
CMD ["/usr/sbin/sshd", "-D", "-e"]
