# HTTP/2 server for tests/test_refused_stream.php (issue #22). Needs python3-h2.
#
# Answers the first call on each connection with an empty message, then
# refuses later calls without processing them:
#   :50061  GOAWAY whose last-stream-id is below the call (a server going away)
#   :50062  RST_STREAM(REFUSED_STREAM) on every later call
#   :50063  RST_STREAM(REFUSED_STREAM) on the second call only
import socket, struct, threading
import h2.config, h2.connection, h2.events, h2.errors


def handle(sock, mode):
    conn = h2.connection.H2Connection(h2.config.H2Configuration(client_side=False))
    conn.initiate_connection()
    sock.sendall(conn.data_to_send())
    calls = 0
    while data := sock.recv(65535):
        for ev in conn.receive_data(data):
            if not isinstance(ev, h2.events.StreamEnded):
                continue
            calls += 1
            if calls == 1 or (mode == 'refused-once' and calls > 2):
                conn.send_headers(ev.stream_id, [(':status', '200'), ('content-type', 'application/grpc')])
                conn.send_data(ev.stream_id, b'\x00' + struct.pack('>I', 0))
                conn.send_headers(ev.stream_id, [('grpc-status', '0')], end_stream=True)
            elif mode == 'goaway':
                conn.close_connection(h2.errors.ErrorCodes.NO_ERROR, last_stream_id=ev.stream_id - 2)
            else:
                conn.reset_stream(ev.stream_id, h2.errors.ErrorCodes.REFUSED_STREAM)
        sock.sendall(conn.data_to_send())


def serve(port, mode):
    srv = socket.create_server(('0.0.0.0', port))
    while True:
        threading.Thread(target=handle, args=(srv.accept()[0], mode), daemon=True).start()


threading.Thread(target=serve, args=(50062, 'refused'), daemon=True).start()
threading.Thread(target=serve, args=(50063, 'refused-once'), daemon=True).start()
serve(50061, 'goaway')
