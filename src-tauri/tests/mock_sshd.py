#!/usr/bin/env python3
"""Server SSH tiruan untuk test E2E Tambat (`cargo test -- --ignored`).

Melayani:
- auth password demo/demo
- exec: perintah dijalankan lewat shell lokal, stdin/stdout/stderr
  tersambung ke channel, exit status diteruskan
- SFTP: dipetakan langsung ke filesystem lokal (realpath, readdir,
  stat/lstat, open/read)

Jalankan: python3 mock_sshd.py   (mendengarkan di 127.0.0.1:2222)
Butuh: pip install paramiko

Env opsional:
  MOCK_SSHD_PORT=2223               porta lain (default 2222)
  MOCK_SSHD_AUTH=keyboard-interactive
      Tiru server yang mematikan `PasswordAuthentication` tapi menyalakan
      `KbdInteractiveAuthentication` — hanya menerima keyboard-interactive,
      dengan satu tantangan "Password: ". Dipakai test E2E
      `ssh::tests::auth_keyboard_interactive_saat_password_dimatikan`.
"""
import os
import socket
import subprocess
import threading

import paramiko
from paramiko import (
    SFTP_OK,
    SFTPAttributes,
    SFTPHandle,
    SFTPServer,
    SFTPServerInterface,
)

HOST = "127.0.0.1"
PORT = int(os.environ.get("MOCK_SSHD_PORT", "2222"))
AUTH = os.environ.get("MOCK_SSHD_AUTH", "password")
USER, PASSWORD = "demo", "demo"


class Handle(SFTPHandle):
    def stat(self):
        try:
            return SFTPAttributes.from_stat(os.fstat(self.readfile.fileno()))
        except OSError as e:
            return SFTPServer.convert_errno(e.errno)


class LocalSftp(SFTPServerInterface):
    """SFTP langsung ke filesystem lokal (cukup untuk test E2E)."""

    def list_folder(self, path):
        try:
            out = []
            for name in os.listdir(path):
                attr = SFTPAttributes.from_stat(os.lstat(os.path.join(path, name)))
                attr.filename = name
                out.append(attr)
            return out
        except OSError as e:
            return SFTPServer.convert_errno(e.errno)

    def stat(self, path):
        try:
            return SFTPAttributes.from_stat(os.stat(path))
        except OSError as e:
            return SFTPServer.convert_errno(e.errno)

    def lstat(self, path):
        try:
            return SFTPAttributes.from_stat(os.lstat(path))
        except OSError as e:
            return SFTPServer.convert_errno(e.errno)

    def mkdir(self, path, attr):
        try:
            os.mkdir(path, getattr(attr, "st_mode", None) or 0o755)
            return SFTP_OK
        except OSError as e:
            return SFTPServer.convert_errno(e.errno)

    def rename(self, oldpath, newpath):
        try:
            os.rename(oldpath, newpath)
            return SFTP_OK
        except OSError as e:
            return SFTPServer.convert_errno(e.errno)

    def remove(self, path):
        try:
            os.remove(path)
            return SFTP_OK
        except OSError as e:
            return SFTPServer.convert_errno(e.errno)

    def rmdir(self, path):
        try:
            os.rmdir(path)
            return SFTP_OK
        except OSError as e:
            return SFTPServer.convert_errno(e.errno)

    def open(self, path, flags, attr):
        try:
            fd = os.open(path, flags, getattr(attr, "st_mode", None) or 0o644)
        except OSError as e:
            return SFTPServer.convert_errno(e.errno)
        if flags & os.O_WRONLY:
            mode = "ab" if flags & os.O_APPEND else "wb"
        elif flags & os.O_RDWR:
            mode = "a+b" if flags & os.O_APPEND else "r+b"
        else:
            mode = "rb"
        handle = Handle(flags)
        handle.filename = path
        handle.readfile = handle.writefile = os.fdopen(fd, mode)
        return handle


def run_exec(channel, command):
    if isinstance(command, bytes):
        command = command.decode()
    proc = subprocess.Popen(
        command,
        shell=True,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    def pump_stdin():
        try:
            while True:
                data = channel.recv(32768)
                if not data:
                    break
                proc.stdin.write(data)
                proc.stdin.flush()
        except Exception:
            pass
        try:
            proc.stdin.close()
        except Exception:
            pass

    def pump_stderr():
        for chunk in iter(lambda: proc.stderr.read(32768), b""):
            channel.sendall_stderr(chunk)

    t_in = threading.Thread(target=pump_stdin, daemon=True)
    t_err = threading.Thread(target=pump_stderr, daemon=True)
    t_in.start()
    t_err.start()
    for chunk in iter(lambda: proc.stdout.read(32768), b""):
        channel.sendall(chunk)
    proc.wait()
    t_err.join()
    try:
        channel.send_exit_status(proc.returncode)
        channel.shutdown_write()
        channel.close()
    except Exception:
        pass


class Server(paramiko.ServerInterface):
    def get_allowed_auths(self, username):
        return AUTH

    def check_auth_password(self, username, password):
        # Dengan MOCK_SSHD_AUTH=keyboard-interactive, tolak apa pun lewat jalur
        # ini — persis seperti server ber-`PasswordAuthentication no`.
        if AUTH != "password":
            return paramiko.AUTH_FAILED
        if (username, password) == (USER, PASSWORD):
            return paramiko.AUTH_SUCCESSFUL
        return paramiko.AUTH_FAILED

    def check_auth_interactive(self, username, submethods):
        if AUTH != "keyboard-interactive":
            return paramiko.AUTH_FAILED
        # (prompt, echo) — echo False = jawabannya rahasia, seperti password.
        return paramiko.InteractiveQuery("", "", ("Password: ", False))

    def check_auth_interactive_response(self, responses):
        if list(responses) == [PASSWORD]:
            return paramiko.AUTH_SUCCESSFUL
        return paramiko.AUTH_FAILED

    def check_channel_request(self, kind, chanid):
        if kind == "session":
            return paramiko.OPEN_SUCCEEDED
        return paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

    def check_channel_pty_request(self, *args):
        return True

    def check_channel_exec_request(self, channel, command):
        threading.Thread(target=run_exec, args=(channel, command), daemon=True).start()
        return True

    def check_channel_shell_request(self, channel):
        threading.Thread(target=run_exec, args=(channel, "sh"), daemon=True).start()
        return True


def handle_conn(sock, host_key):
    transport = paramiko.Transport(sock)
    transport.add_server_key(host_key)
    transport.set_subsystem_handler("sftp", SFTPServer, LocalSftp)
    transport.start_server(server=Server())
    transport.join()


def main():
    host_key = paramiko.RSAKey.generate(2048)
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind((HOST, PORT))
    listener.listen(8)
    print(f"mock sshd mendengarkan di {HOST}:{PORT} (user {USER}/{PASSWORD})")
    while True:
        sock, _ = listener.accept()
        threading.Thread(target=handle_conn, args=(sock, host_key), daemon=True).start()


if __name__ == "__main__":
    main()
