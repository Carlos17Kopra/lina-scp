FROM debian:trixie-slim
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends openssh-server sudo \
 && rm -rf /var/lib/apt/lists/* \
 && mkdir -p /run/sshd /etc/ssh/authorized_keys \
 && sed -i 's/^session\s\+required\s\+pam_loginuid.so/session optional pam_loginuid.so/' /etc/pam.d/sshd \
 && useradd -m -s /bin/bash pwuser && echo 'pwuser:pwpass' | chpasswd \
 && useradd -m -s /bin/bash keyuser && echo 'keyuser:unused-Kq81' | chpasswd \
 && useradd -m -s /bin/bash kbduser && echo 'kbduser:kbdpass' | chpasswd \
 && useradd -m -s /bin/bash mfauser && echo 'mfauser:mfapass' | chpasswd \
 && useradd -m -s /bin/bash mfpwuser && echo 'mfpwuser:mfpwpass' | chpasswd \
 && echo 'pwuser ALL=(root) NOPASSWD: /usr/lib/openssh/sftp-server' > /etc/sudoers.d/sftp \
 && chmod 440 /etc/sudoers.d/sftp
COPY sshd_config /etc/ssh/sshd_config
COPY keyuser.authorized_keys /etc/ssh/authorized_keys/keyuser
RUN cp /etc/ssh/authorized_keys/keyuser /etc/ssh/authorized_keys/mfauser \
 && cp /etc/ssh/authorized_keys/keyuser /etc/ssh/authorized_keys/mfpwuser \
 && chmod 644 /etc/ssh/authorized_keys/keyuser /etc/ssh/authorized_keys/mfauser /etc/ssh/authorized_keys/mfpwuser \
 && sshd -t
EXPOSE 22
CMD ["/usr/sbin/sshd", "-D", "-e"]
