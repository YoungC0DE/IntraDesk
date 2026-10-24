import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/common/widgets/peer_card.dart';
import 'package:flutter_hbb/models/peer_model.dart';
import 'package:flutter_hbb/models/peer_tab_model.dart';
import 'package:flutter_test/flutter_test.dart';

Peer _lanPeer(Map<String, dynamic> extra) => Peer.fromJson({
      'id': '123456789',
      'username': 'maria',
      'hostname': 'PC-FINANCEIRO',
      'platform': 'Windows',
      ...extra,
    });

void main() {
  test('Peer.fromJson reads the LAN ip and version sent by get_lan_peers', () {
    final peer = _lanPeer({'ip': '192.168.0.12', 'version': '1.4.0'});
    expect(peer.ip, '192.168.0.12');
    expect(peer.version, '1.4.0');
    expect(Peer.fromJson(peer.toJson()).ip, '192.168.0.12');
    expect(_lanPeer({}).ip, '');
  });

  test('peer card shows the computer name, user and IP', () {
    final peer = _lanPeer({'ip': '192.168.0.12'});
    expect(peerCardTitle(peer), 'PC-FINANCEIRO');
    expect(peerCardSubtitle(peer), 'maria · 192.168.0.12');
    expect(peerCardSubtitle(peer, hideUsername: true), '192.168.0.12');
    // Without an IP (e.g. a recent session), the ID is shown instead.
    expect(peerCardSubtitle(_lanPeer({})), 'maria · 123 456 789');
  });

  test('network search matches the IP', () async {
    final peer = _lanPeer({'ip': '192.168.0.12'});
    expect(await matchPeer('168.0.12', peer, PeerTabIndex.lan), isTrue);
    expect(await matchPeer('10.0.0', peer, PeerTabIndex.lan), isFalse);
  });

  test('connect field resolves a computer name to its ID', () {
    final peers = [_lanPeer({'ip': '192.168.0.12'})];
    expect(resolveConnectTarget('pc-financeiro', peers), '123456789');
    expect(resolveConnectTarget('192.168.0.50', peers), '192.168.0.50');
    expect(resolveConnectTarget('123456789', peers), '123456789');
  });
}
