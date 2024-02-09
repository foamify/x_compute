import 'dart:math';
import 'dart:ui';

import 'package:fast_immutable_collections/fast_immutable_collections.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:x_compute/src/rust/api/simple.dart';
import 'package:x_compute/src/rust/frb_generated.dart';

Future<void> main() async {
  await RustLib.init();
  runApp(const MyApp());
}

class MyApp extends StatefulWidget {
  const MyApp({super.key});

  @override
  State<MyApp> createState() => _MyAppState();
}

class _MyAppState extends State<MyApp> {
  final pointsInside = ValueNotifier(IList<F32Array2>());
  final rect = ValueNotifier(ComputeRect(
    min: F32Array2(Float32List.fromList([0.0, 0.0])),
    max: F32Array2(Float32List.fromList([0.0, 0.0])),
  ));

  @override
  Widget build(BuildContext context) {
    final points = List.generate(10000 * 4, (index) {
      final size = MediaQuery.sizeOf(context);
      return [
        Random().nextInt(size.width.toInt()),
        Random().nextInt(
          size.height.toInt(),
        )
      ].map((e) => e.toDouble()).toList();
    }).map((e) => F32Array2(Float32List.fromList(e))).toList();

    return MaterialApp(
      home: Scaffold(
        backgroundColor: Colors.grey,
        body: Stack(
          children: [
            // CustomPaint(
            //   painter: PointPainter(points, Colors.blue),
            // ),
            ValueListenableBuilder(
                valueListenable: pointsInside,
                builder: (context, value, child) {
                  return CustomPaint(
                    painter: PointPainter(value.unlockView, Colors.blueGrey),
                  );
                }),
            ValueListenableBuilder(
                valueListenable: rect,
                builder: (context, value, child) {
                  return CustomPaint(
                    painter: RectPainter(value),
                  );
                }),
            MouseRegion(
              onHover: (event) {
                final minRect = [0.0, 0.0];
                final maxRect = [event.position.dx, event.position.dy];
                // print(maxRect);
                rect.value = ComputeRect(
                  min: F32Array2(Float32List.fromList(minRect)),
                  max: F32Array2(
                    Float32List.fromList(maxRect),
                  ),
                );
                // final now = DateTime.now();
                SchedulerBinding.instance
                    .scheduleTask(
                  () => runCompute(
                    points: points,
                    rect: rect.value,
                  ),
                  Priority.animation,
                )
                    .then(
                  (value) {
                    // print('Elapsed: ${DateTime.now().difference(now)}');
                    pointsInside.value.clear().flush;
                    pointsInside.value = (value ?? []).lock;
                  },
                );
              },
              child: const Center(
                  child: Text('Hover to calculate points inside rect')),
            ),
          ],
        ),
      ),
    );
  }
}

class PointPainter extends CustomPainter {
  PointPainter(this.points, this.color);
  final List<F32Array2> points;
  final Color color;

  @override
  void paint(Canvas canvas, Size size) {
    final view =
        points.map((point) => Offset(point[0], point[1])).toIList().unlockView;
    canvas.drawPoints(
      PointMode.points,
      view,
      Paint()
        ..color = color
        ..strokeWidth = 5
        ..strokeCap = StrokeCap.round,
    );
    canvas.drawPoints(
      PointMode.points,
      view,
      Paint()
        ..color = Colors.white
        ..strokeWidth = 1,
    );
  }

  @override
  bool shouldRepaint(covariant CustomPainter oldDelegate) => true;
}

class RectPainter extends CustomPainter {
  RectPainter(this.rect);
  final ComputeRect rect;

  @override
  void paint(Canvas canvas, Size size) {
    final paint = Paint()
          //
          ..color = Colors.redAccent.withOpacity(.2)
        // ..strokeWidth = 1
        // ..style = PaintingStyle.stroke
        //
        ;

    canvas.drawRect(
        Rect.fromPoints(
            Offset(rect.min[0], rect.min[1]), Offset(rect.max[0], rect.max[1])),
        paint);
  }

  @override
  bool shouldRepaint(covariant CustomPainter oldDelegate) => true;
}
