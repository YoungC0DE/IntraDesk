import 'dart:convert';

import 'package:flutter_hbb/common/widgets/peers_view.dart';
import 'package:flutter_hbb/models/peer_model.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:flutter_test/flutter_test.dart';

// The payload `main_load_lan_peers` (src/flutter_ffi.rs) pushes, built from
// `get_lan_peers` (src/ui_interface.rs).
Map<String, dynamic> _lanEvent(List<Map<String, String>> peers) => {
      'name': LoadEvent.lan,
      'peers': jsonEncode(peers),
    };

void main() {
  late Peers model;

  setUp(() {
    model = Peers(
        name: PeersModelName.lan, loadEvent: LoadEvent.lan, getInitPeers: null);
  });

  tearDown(() => model.dispose());

  test('the network tab lists the computers the core discovered', () async {
    var notified = 0;
    model.addListener(() => notified++);

    final handled = await platformFFI.tryHandle(_lanEvent([
      {
        'id': '111111111',
        'username': 'maria',
        'hostname': 'PC-FINANCEIRO',
        'platform': 'Windows',
        'ip': '192.168.0.12',
        'version': '1.4.0',
      },
      {
        'id': '222222222',
        'username': 'joao',
        'hostname': 'PC-RH',
        'platform': 'Windows',
        'ip': '192.168.0.20',
        'version': '1.4.0',
      },
    ]));

    expect(handled, isTrue);
    expect(notified, 1);
    expect(model.getPeersCount(), 2);
    expect(model.peers.map((p) => p.hostname), ['PC-FINANCEIRO', 'PC-RH']);
    expect(model.peers.map((p) => p.ip), ['192.168.0.12', '192.168.0.20']);
    expect(model.peers.first.username, 'maria');
  });

  test('a new discovery round replaces the list', () async {
    await platformFFI.tryHandle(_lanEvent([
      {'id': '111111111', 'hostname': 'PC-FINANCEIRO', 'ip': '192.168.0.12'},
    ]));
    await platformFFI.tryHandle(_lanEvent([
      {'id': '333333333', 'hostname': 'PC-NOVO', 'ip': '192.168.0.30'},
    ]));
    expect(model.peers.map((p) => p.id), ['333333333']);

    await platformFFI.tryHandle(_lanEvent([]));
    expect(model.getPeersCount(), 0);
  });
}
