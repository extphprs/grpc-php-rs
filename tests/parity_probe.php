<?php
/**
 * ext-grpc parity probe.
 *
 * Runs the same wire-level scenarios under any Grpc\ extension and prints a
 * normalized JSON document of the observable results. Run it once under the
 * official ext-grpc and once under grpc-php-rs against the same test server;
 * the outputs must be identical (`./test.sh parity` does the diff).
 *
 * Only implementation-independent observables are emitted: status codes,
 * message payload bytes, and server-controlled metadata/status text. Free-text
 * messages of locally generated errors (deadline, connect failure) are
 * intentionally excluded — C-core and tonic word them differently.
 */
declare(strict_types=1);

const TARGET = 'localhost:50051';
const DEAD_TARGET = '127.0.0.1:59999';

function encode_varint(int $value): string {
    $buf = '';
    while ($value > 0x7f) {
        $buf .= chr(($value & 0x7f) | 0x80);
        $value >>= 7;
    }
    return $buf . chr($value & 0x7f);
}

function encode_payload(string $data): string {
    if ($data === '') return '';
    return "\x0a" . encode_varint(strlen($data)) . $data;
}

/** Decode the `body` field of a Payload message. */
function decode_body(?string $wire): ?string {
    if ($wire === null) return null;
    if ($wire === '') return '';
    if ($wire[0] !== "\x0a") return '<unparseable:' . bin2hex(substr($wire, 0, 8)) . '>';
    $len = 0;
    $shift = 0;
    $i = 1;
    while (true) {
        $b = ord($wire[$i]);
        $len |= ($b & 0x7f) << $shift;
        $i++;
        if (!($b & 0x80)) break;
        $shift += 7;
    }
    return substr($wire, $i, $len);
}

/** Curated metadata view: hex values for keys both implementations must surface. */
function curated_metadata(?array $metadata): array {
    $out = [];
    foreach (['x-test-binary-bin', 'grpc-status-details-bin'] as $key) {
        if (isset($metadata[$key])) {
            $out[$key] = array_map('bin2hex', $metadata[$key]);
        }
    }
    return $out;
}

function norm_status(object $status): array {
    return [
        'code' => $status->code,
        // `details` is server-controlled (grpc-message) for remote statuses;
        // only include it when the status came from the server (code 13 in
        // these scenarios) so local error wording differences don't diverge.
        'details' => $status->code === Grpc\STATUS_INTERNAL ? $status->details : '<local>',
        'trailing' => curated_metadata($status->metadata ?? []),
    ];
}

function scenario(string $name, callable $fn): array {
    try {
        return $fn();
    } catch (Throwable $e) {
        return ['exception' => get_class($e), 'code' => $e->getCode()];
    }
}

$channel = new Grpc\Channel(TARGET, ['credentials' => Grpc\ChannelCredentials::createInsecure()]);
$result = [];

// ---------------------------------------------------------------------------
$result['unary_ok'] = scenario('unary_ok', function () use ($channel) {
    $call = new Grpc\Call($channel, '/grpc.testing.TestService/Echo', Grpc\Timeval::infFuture());
    $r = $call->startBatch([
        Grpc\OP_SEND_INITIAL_METADATA => [],
        Grpc\OP_SEND_MESSAGE => ['message' => encode_payload('parity')],
        Grpc\OP_SEND_CLOSE_FROM_CLIENT => true,
        Grpc\OP_RECV_INITIAL_METADATA => true,
        Grpc\OP_RECV_MESSAGE => true,
        Grpc\OP_RECV_STATUS_ON_CLIENT => true,
    ]);
    return [
        'message_body' => decode_body($r->message),
        'initial_metadata' => curated_metadata($r->metadata ?? []),
        'status' => norm_status($r->status),
    ];
});

// ---------------------------------------------------------------------------
$result['empty_response'] = scenario('empty_response', function () use ($channel) {
    $call = new Grpc\Call($channel, '/grpc.testing.TestService/EmptyResponse', Grpc\Timeval::infFuture());
    $r = $call->startBatch([
        Grpc\OP_SEND_INITIAL_METADATA => [],
        Grpc\OP_SEND_MESSAGE => ['message' => encode_payload('x')],
        Grpc\OP_SEND_CLOSE_FROM_CLIENT => true,
        Grpc\OP_RECV_INITIAL_METADATA => true,
        Grpc\OP_RECV_MESSAGE => true,
        Grpc\OP_RECV_STATUS_ON_CLIENT => true,
    ]);
    return [
        // Historic issue #7: empty response must be '' (string), not null
        'message_type' => $r->message === null ? 'null' : 'string',
        'message_len' => $r->message === null ? -1 : strlen($r->message),
        'status_code' => $r->status->code,
    ];
});

// ---------------------------------------------------------------------------
$result['large_response'] = scenario('large_response', function () use ($channel) {
    $call = new Grpc\Call($channel, '/grpc.testing.TestService/LargeResponse', Grpc\Timeval::infFuture());
    $r = $call->startBatch([
        Grpc\OP_SEND_INITIAL_METADATA => [],
        Grpc\OP_SEND_MESSAGE => ['message' => encode_payload('x')],
        Grpc\OP_SEND_CLOSE_FROM_CLIENT => true,
        Grpc\OP_RECV_INITIAL_METADATA => true,
        Grpc\OP_RECV_MESSAGE => true,
        Grpc\OP_RECV_STATUS_ON_CLIENT => true,
    ]);
    $body = decode_body($r->message);
    return [
        'body_len' => strlen((string)$body),
        'body_md5' => md5((string)$body),
        'status_code' => $r->status->code,
    ];
});

// ---------------------------------------------------------------------------
$result['unary_error_rich_status'] = scenario('unary_error_rich_status', function () use ($channel) {
    $call = new Grpc\Call($channel, '/grpc.testing.TestService/ErrorResponse', Grpc\Timeval::infFuture());
    $r = $call->startBatch([
        Grpc\OP_SEND_INITIAL_METADATA => [],
        Grpc\OP_SEND_MESSAGE => ['message' => encode_payload('x')],
        Grpc\OP_SEND_CLOSE_FROM_CLIENT => true,
        Grpc\OP_RECV_INITIAL_METADATA => true,
        Grpc\OP_RECV_MESSAGE => true,
        Grpc\OP_RECV_STATUS_ON_CLIENT => true,
    ]);
    return [
        'message' => $r->message === null ? 'null' : bin2hex($r->message),
        'status' => norm_status($r->status),
    ];
});

// ---------------------------------------------------------------------------
$result['server_streaming_ok'] = scenario('server_streaming_ok', function () use ($channel) {
    $call = new Grpc\Call($channel, '/grpc.testing.TestService/StreamEcho', Grpc\Timeval::infFuture());
    $call->startBatch([
        Grpc\OP_SEND_INITIAL_METADATA => [],
        Grpc\OP_SEND_MESSAGE => ['message' => encode_payload('abc')],
        Grpc\OP_SEND_CLOSE_FROM_CLIENT => true,
        Grpc\OP_RECV_INITIAL_METADATA => true,
    ]);
    $messages = [];
    while (true) {
        $r = $call->startBatch([Grpc\OP_RECV_MESSAGE => true]);
        if ($r->message === null) break;
        $messages[] = decode_body($r->message);
    }
    $s = $call->startBatch([Grpc\OP_RECV_STATUS_ON_CLIENT => true]);
    return ['messages' => $messages, 'status_code' => $s->status->code];
});

// ---------------------------------------------------------------------------
$result['server_streaming_error'] = scenario('server_streaming_error', function () use ($channel) {
    $call = new Grpc\Call($channel, '/grpc.testing.TestService/StreamEcho', Grpc\Timeval::infFuture());
    $call->startBatch([
        Grpc\OP_SEND_INITIAL_METADATA => [],
        Grpc\OP_SEND_MESSAGE => ['message' => encode_payload('stream-status-error')],
        Grpc\OP_SEND_CLOSE_FROM_CLIENT => true,
        Grpc\OP_RECV_INITIAL_METADATA => true,
    ]);
    $r = $call->startBatch([Grpc\OP_RECV_MESSAGE => true]);
    $s = $call->startBatch([Grpc\OP_RECV_STATUS_ON_CLIENT => true]);
    return [
        'message' => $r->message === null ? 'null' : bin2hex($r->message),
        'status' => norm_status($s->status),
    ];
});

// ---------------------------------------------------------------------------
$result['client_streaming'] = scenario('client_streaming', function () use ($channel) {
    $call = new Grpc\Call($channel, '/grpc.testing.TestService/CollectPayloads', Grpc\Timeval::infFuture());
    $call->startBatch([Grpc\OP_SEND_INITIAL_METADATA => []]);
    foreach (['aa', 'bb', 'cc'] as $part) {
        $call->startBatch([Grpc\OP_SEND_MESSAGE => ['message' => encode_payload($part)]]);
    }
    $r = $call->startBatch([
        Grpc\OP_SEND_CLOSE_FROM_CLIENT => true,
        Grpc\OP_RECV_INITIAL_METADATA => true,
        Grpc\OP_RECV_MESSAGE => true,
        Grpc\OP_RECV_STATUS_ON_CLIENT => true,
    ]);
    return [
        'collected' => decode_body($r->message),
        'status_code' => $r->status->code,
    ];
});

// ---------------------------------------------------------------------------
$result['bidi_streaming'] = scenario('bidi_streaming', function () use ($channel) {
    $call = new Grpc\Call($channel, '/grpc.testing.TestService/BidiEcho', Grpc\Timeval::infFuture());
    $call->startBatch([Grpc\OP_SEND_INITIAL_METADATA => []]);
    $echoed = [];
    foreach (['one', 'two'] as $i => $word) {
        $call->startBatch([Grpc\OP_SEND_MESSAGE => ['message' => encode_payload($word)]]);
        // Like grpc/grpc's BidiStreamingCall::read(), ask for initial metadata
        // with the first read. ext-grpc 1.85 never completes a read on a call
        // whose initial metadata was not requested.
        $r = $call->startBatch([Grpc\OP_RECV_MESSAGE => true] + ($i === 0 ? [Grpc\OP_RECV_INITIAL_METADATA => true] : []));
        $echoed[] = decode_body($r->message);
    }
    $call->startBatch([Grpc\OP_SEND_CLOSE_FROM_CLIENT => true]);
    $end = $call->startBatch([Grpc\OP_RECV_MESSAGE => true]);
    $s = $call->startBatch([Grpc\OP_RECV_STATUS_ON_CLIENT => true]);
    return [
        'echoed' => $echoed,
        'end_of_stream' => $end->message === null ? 'null' : bin2hex($end->message),
        'status_code' => $s->status->code,
    ];
});

// ---------------------------------------------------------------------------
$result['deadline_expired'] = scenario('deadline_expired', function () use ($channel) {
    // new Timeval(1) is a 1µs timespan from now. C-core keeps deadlines in whole
    // milliseconds, so a loopback call still completes.
    $call = new Grpc\Call($channel, '/grpc.testing.TestService/Echo', new Grpc\Timeval(1));
    $r = $call->startBatch([
        Grpc\OP_SEND_INITIAL_METADATA => [],
        Grpc\OP_SEND_MESSAGE => ['message' => encode_payload('late')],
        Grpc\OP_SEND_CLOSE_FROM_CLIENT => true,
        Grpc\OP_RECV_INITIAL_METADATA => true,
        Grpc\OP_RECV_MESSAGE => true,
        Grpc\OP_RECV_STATUS_ON_CLIENT => true,
    ]);
    return ['status_code' => $r->status->code, 'message' => $r->message === null ? 'null' : 'present'];
});

// ---------------------------------------------------------------------------
$result['unavailable'] = scenario('unavailable', function () {
    $dead = new Grpc\Channel(DEAD_TARGET, ['credentials' => Grpc\ChannelCredentials::createInsecure()]);
    // Bounded deadline: with an infinite one, C-core retries the dead
    // endpoint forever and the probe never terminates.
    $deadline = Grpc\Timeval::now()->add(new Grpc\Timeval(2_000_000));
    $call = new Grpc\Call($dead, '/grpc.testing.TestService/Echo', $deadline);
    $r = $call->startBatch([
        Grpc\OP_SEND_INITIAL_METADATA => [],
        Grpc\OP_SEND_MESSAGE => ['message' => encode_payload('x')],
        Grpc\OP_SEND_CLOSE_FROM_CLIENT => true,
        Grpc\OP_RECV_INITIAL_METADATA => true,
        Grpc\OP_RECV_MESSAGE => true,
        Grpc\OP_RECV_STATUS_ON_CLIENT => true,
    ]);
    return ['status_code' => $r->status->code];
});

// ---------------------------------------------------------------------------
$result['cancel_mid_stream'] = scenario('cancel_mid_stream', function () use ($channel) {
    $call = new Grpc\Call($channel, '/grpc.testing.TestService/StreamEcho', Grpc\Timeval::infFuture());
    $call->startBatch([
        Grpc\OP_SEND_INITIAL_METADATA => [],
        Grpc\OP_SEND_MESSAGE => ['message' => encode_payload('repeat:1000')],
        Grpc\OP_SEND_CLOSE_FROM_CLIENT => true,
        Grpc\OP_RECV_INITIAL_METADATA => true,
    ]);
    $r = $call->startBatch([Grpc\OP_RECV_MESSAGE => true]);
    $got_first = $r->message !== null;
    $call->cancel();
    $s = $call->startBatch([Grpc\OP_RECV_STATUS_ON_CLIENT => true]);
    return ['got_first_message' => $got_first, 'status_code' => $s->status->code];
});

// ---------------------------------------------------------------------------
// Argument types: which calls are accepted, and the exception class, code and
// message of those that are not — under strict_types and without it. The
// cases live in parity_arg_cases.inc so both modes run the same source.
function arg_type_probes(bool $strict): array {
    $source = file_get_contents(__DIR__ . '/parity_arg_cases.inc');
    return eval(($strict ? 'declare(strict_types=1);' : '') . $source);
}
$result['arg_types_strict'] = arg_type_probes(true);
$result['arg_types_weak'] = arg_type_probes(false);
// If eval ignored the declare, both modes would silently test the same thing.
if ($result['arg_types_strict'] === $result['arg_types_weak']) {
    fwrite(STDERR, "parity probe sentinel: strict and weak argument probes are identical\n");
    exit(1);
}

// ---------------------------------------------------------------------------
// gRPC name syntax targets (issue #20).
foreach (['dns:///localhost:50051', 'dns:localhost:50051', 'ipv4:127.0.0.1:50051'] as $target) {
    $result['target ' . $target] = scenario('target ' . $target, function () use ($target) {
        $ch = new Grpc\Channel($target, ['credentials' => Grpc\ChannelCredentials::createInsecure()]);
        $call = new Grpc\Call($ch, '/grpc.testing.TestService/Echo',
            Grpc\Timeval::now()->add(new Grpc\Timeval(5_000_000)));
        $r = $call->startBatch([
            Grpc\OP_SEND_INITIAL_METADATA => [],
            Grpc\OP_SEND_MESSAGE => ['message' => encode_payload('hi')],
            Grpc\OP_SEND_CLOSE_FROM_CLIENT => true,
            Grpc\OP_RECV_INITIAL_METADATA => true,
            Grpc\OP_RECV_MESSAGE => true,
            Grpc\OP_RECV_STATUS_ON_CLIENT => true,
        ]);
        return ['status_code' => $r->status->code, 'target' => $ch->getTarget()];
    });
}

// Sentinel: if the baseline scenario shows no working RPC, this probe run is
// meaningless (dead server / dead port) and identical failure on both sides
// must NOT diff as "parity". Fail loudly instead.
if (($result['unary_ok']['status']['code'] ?? -1) !== 0) {
    fwrite(STDERR, "parity probe sentinel: baseline unary failed — no valid comparison possible\n");
    exit(1);
}

echo json_encode($result, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES), "\n";
