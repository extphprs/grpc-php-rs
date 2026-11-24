<?php
// NO declare(strict_types=1) here — deliberately. Real client libraries run
// in weak typing mode; the parity ledger's weak-mode-coercion case includes
// this file so its probes execute with genuine weak-mode coercion semantics
// (the ledger file itself is strict, which would change what is measured).

function weak_mode_probes(Grpc\Channel $ch): string {
    $f = function (callable $c) {
        try { $c(); return 'ok'; }
        catch (Throwable $e) {
            $short = substr(strrchr('\\' . get_class($e), '\\'), 1);
            return 'throw:' . $short;
        }
    };
    return sprintf('int=%s null=%s str=%s',
        $f(fn() => $ch->getConnectivityState(1)),
        $f(fn() => @$ch->getConnectivityState(null)),
        $f(fn() => new Grpc\Timeval('1000000')));
}

/** Scalars that ext-grpc's zend_parse_parameters coerces in weak mode. */
function weak_scalar_probes(Grpc\Channel $ch): string {
    $tv = new Grpc\Timeval(1000);
    $soon = fn() => Grpc\Timeval::now()->add(new Grpc\Timeval(1000));
    $ok = [
        fn() => Grpc\ChannelCredentials::createSsl(42, 3.14, true),
        fn() => Grpc\ChannelCredentials::createSsl(null, null, null),
        fn() => Grpc\ChannelCredentials::setDefaultRootsPem(42),
        fn() => new Grpc\Channel(42, []),
        fn() => new Grpc\Call($ch, 42, $tv),
        fn() => new Grpc\Call($ch, '/m', $tv, 42),
        fn() => $ch->watchConnectivityState('1', $soon()),
        fn() => $ch->watchConnectivityState(1.0, $soon()),
        fn() => new Grpc\Timeval(1.5),
    ];
    try {
        foreach ($ok as $i => $fn) {
            try { $fn(); }
            catch (Throwable $e) { return "case{$i}=" . get_class($e); }
        }
        return 'all-ok';
    } finally {
        Grpc\ChannelCredentials::invalidateDefaultRootsPem();
    }
}
