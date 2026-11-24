<?php
/**
 * Issue #22: a call the server refused without processing it (GOAWAY below
 * the stream id, or RST_STREAM(REFUSED_STREAM)) is retried once, transparently;
 * if it is refused again it reports UNAVAILABLE. Runs against
 * tests/refusing_server.py.
 */
declare(strict_types=1);

const METHOD = '/test.Svc/Call';

function channel(int $port): Grpc\Channel {
    return new Grpc\Channel("127.0.0.1:$port", [
        'credentials' => Grpc\ChannelCredentials::createInsecure(),
        'force_new' => true,
    ]);
}

function deadline(): Grpc\Timeval {
    return Grpc\Timeval::now()->add(new Grpc\Timeval(2_000_000));
}

function unary(Grpc\Channel $ch): int {
    $call = new Grpc\Call($ch, METHOD, deadline());
    $r = $call->startBatch([
        Grpc\OP_SEND_INITIAL_METADATA => [],
        Grpc\OP_SEND_MESSAGE => ['message' => ''],
        Grpc\OP_SEND_CLOSE_FROM_CLIENT => true,
        Grpc\OP_RECV_INITIAL_METADATA => true,
        Grpc\OP_RECV_MESSAGE => true,
        Grpc\OP_RECV_STATUS_ON_CLIENT => true,
    ]);
    return $r->status->code;
}

function server_streaming(Grpc\Channel $ch): int {
    $call = new Grpc\Call($ch, METHOD, deadline());
    $call->startBatch([
        Grpc\OP_SEND_INITIAL_METADATA => [],
        Grpc\OP_SEND_MESSAGE => ['message' => ''],
        Grpc\OP_SEND_CLOSE_FROM_CLIENT => true,
        Grpc\OP_RECV_INITIAL_METADATA => true,
    ]);
    $call->startBatch([Grpc\OP_RECV_MESSAGE => true]);
    return $call->startBatch([Grpc\OP_RECV_STATUS_ON_CLIENT => true])->status->code;
}

$cases = [
    // name => [port, second call, expected status of the second call]
    'GOAWAY, unary' => [50061, 'unary', Grpc\STATUS_OK],
    'GOAWAY, server streaming' => [50061, 'server_streaming', Grpc\STATUS_OK],
    'REFUSED_STREAM once, unary' => [50063, 'unary', Grpc\STATUS_OK],
    'REFUSED_STREAM once, server streaming' => [50063, 'server_streaming', Grpc\STATUS_OK],
    'REFUSED_STREAM every time, unary' => [50062, 'unary', Grpc\STATUS_UNAVAILABLE],
];

$failed = 0;
foreach ($cases as $name => [$port, $second, $expected]) {
    $ch = channel($port);
    $first = unary($ch);
    $got = $second($ch);
    $ok = $first === Grpc\STATUS_OK && $got === $expected;
    $failed += $ok ? 0 : 1;
    printf("  %s %s: first=%d second=%d (expected %d)\n", $ok ? '✓' : '✗', $name, $first, $got, $expected);
}
exit($failed === 0 ? 0 : 1);
