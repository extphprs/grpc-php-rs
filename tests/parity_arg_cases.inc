// Argument-type probes, evaluated twice by arg_type_probes() in
// parity_probe.php: once under declare(strict_types=1) and once without.
// No <?php tag and no declare here — the caller prepends them.
$creds = ['credentials' => Grpc\ChannelCredentials::createInsecure()];
$ch = new Grpc\Channel('localhost:50051', $creds);
$tv = new Grpc\Timeval(1000);
$soon = fn() => Grpc\Timeval::now()->add(new Grpc\Timeval(1000));
$call = new Grpc\Call($ch, '/m', $tv);
$ssl = Grpc\ChannelCredentials::createSsl();
$cc = Grpc\CallCredentials::createFromPlugin(fn($u) => []);
$cases = [
    'Channel(42, 42)' => fn() => new Grpc\Channel(42, 42),
    'Channel(42, args)' => fn() => new Grpc\Channel(42, $creds),
    'Channel([], args)' => fn() => new Grpc\Channel([], $creds),
    'getConnectivityState(1)' => fn() => $ch->getConnectivityState(1),
    'getConnectivityState(null)' => fn() => $ch->getConnectivityState(null),
    'getConnectivityState([])' => fn() => $ch->getConnectivityState([]),
    'watchConnectivityState("x", tv)' => fn() => $ch->watchConnectivityState('x', $soon()),
    'watchConnectivityState("1", tv)' => fn() => $ch->watchConnectivityState('1', $soon()),
    'watchConnectivityState(1.0, tv)' => fn() => $ch->watchConnectivityState(1.0, $soon()),
    'watchConnectivityState(1, 42)' => fn() => $ch->watchConnectivityState(1, 42),
    'Call(42, m, tv)' => fn() => new Grpc\Call(42, '/m', $tv),
    'Call(ch, 42, tv)' => fn() => new Grpc\Call($ch, 42, $tv),
    'Call(ch, m, 42)' => fn() => new Grpc\Call($ch, '/m', 42),
    'Call(ch, m, tv, 42)' => fn() => new Grpc\Call($ch, '/m', $tv, 42),
    'Call(ch, m, tv, [])' => fn() => new Grpc\Call($ch, '/m', $tv, []),
    'startBatch("x")' => fn() => $call->startBatch('x'),
    'setCredentials(null)' => fn() => $call->setCredentials(null),
    'createSsl([])' => fn() => Grpc\ChannelCredentials::createSsl([]),
    'createSsl(42)' => fn() => Grpc\ChannelCredentials::createSsl(42),
    'createSsl(3.14)' => fn() => Grpc\ChannelCredentials::createSsl(3.14),
    'createSsl(true)' => fn() => Grpc\ChannelCredentials::createSsl(true),
    'createSsl(null, null, null)' => fn() => Grpc\ChannelCredentials::createSsl(null, null, null),
    'ChannelCredentials::createComposite(42, cc)' => fn() => Grpc\ChannelCredentials::createComposite(42, $cc),
    'ChannelCredentials::createComposite(ssl, 42)' => fn() => Grpc\ChannelCredentials::createComposite($ssl, 42),
    'setDefaultRootsPem([])' => fn() => Grpc\ChannelCredentials::setDefaultRootsPem([]),
    'setDefaultRootsPem(42)' => fn() => Grpc\ChannelCredentials::setDefaultRootsPem(42),
    'createFromPlugin(42)' => fn() => Grpc\CallCredentials::createFromPlugin(42),
    'CallCredentials::createComposite(42, cc)' => fn() => Grpc\CallCredentials::createComposite(42, $cc),
    'Timeval("1000")' => fn() => new Grpc\Timeval('1000'),
    'Timeval(1.5)' => fn() => new Grpc\Timeval(1.5),
    'Timeval("x")' => fn() => new Grpc\Timeval('x'),
    'Timeval([])' => fn() => new Grpc\Timeval([]),
    'add("x")' => fn() => $tv->add('x'),
    'subtract("x")' => fn() => $tv->subtract('x'),
    'compare("x", tv)' => fn() => Grpc\Timeval::compare('x', $tv),
    'similar("x", tv, tv)' => fn() => Grpc\Timeval::similar('x', $tv, $tv),
];
$out = [];
foreach ($cases as $name => $fn) {
    try {
        @$fn();
        $out[$name] = 'ok';
    } catch (Throwable $e) {
        $out[$name] = get_class($e) . '(' . $e->getCode() . '): ' . $e->getMessage();
    }
}
Grpc\ChannelCredentials::invalidateDefaultRootsPem();
return $out;
