import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:x_compute/src/rust/api/compute.dart';
import 'package:x_compute/src/rust/frb_generated.dart';

Future<void> main() async {
  await RustLib.init();
  runApp(const MyApp());
}

class MyApp extends StatelessWidget {
  const MyApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      home: Scaffold(
        appBar: AppBar(title: const Text('flutter_rust_bridge quickstart')),
        body: Center(
          child: FilledButton(
            onPressed: () {
              final points = List.generate(
                      500 * 4, (index) => [index.toDouble(), index.toDouble()])
                  .map((e) => F32Array2(Float32List.fromList(e)))
                  .toList();
              final minRect = [0.5, 0.2];
              final maxRect = [100.0, 100.0];
              final rect = ComputeRect(
                min: F32Array2(Float32List.fromList(minRect)),
                max: F32Array2(
                  Float32List.fromList(maxRect),
                ),
              );
              final now = DateTime.now();
              runCollatz(
                points: points,
                rect: rect,
              ).then((value) {
                print('Elapsed ${DateTime.now().difference(now)}');
              });
            },
            child: const Text('Action: Call Rust GPU Compute'),
          ),
        ),
      ),
    );
  }
}
