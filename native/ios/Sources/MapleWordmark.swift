import SwiftUI

struct MapleWordmark: View {
    var color: Color = .pebble50
    var height: CGFloat = 28

    private let aspectRatio: CGFloat = 248.0 / 48.0

    var body: some View {
        GeometryReader { _ in
            Canvas { context, size in
                let scale = size.height / 48.0
                let transform = CGAffineTransform(scaleX: scale, y: scale)

                let paths: [CGPath] = [
                    {
                        let p = CGMutablePath()
                        p.move(to: CGPoint(x: 0, y: 41.2408))
                        p.addLine(to: CGPoint(x: 0, y: 6.06562))
                        p.addCurve(to: CGPoint(x: 10.1942, y: 1.4443), control1: CGPoint(x: 0, y: 0.652098), control2: CGPoint(x: 5.77774, y: -1.72459))
                        p.addLine(to: CGPoint(x: 26.9158, y: 15.561))
                        p.addLine(to: CGPoint(x: 43.6375, y: 1.4443))
                        p.addCurve(to: CGPoint(x: 53.8317, y: 6.06562), control1: CGPoint(x: 48.0567, y: -1.72459), control2: CGPoint(x: 53.8317, y: 0.652098))
                        p.addLine(to: CGPoint(x: 53.8317, y: 41.2408))
                        p.addCurve(to: CGPoint(x: 47.0534, y: 47.9972), control1: CGPoint(x: 53.8317, y: 44.9716), control2: CGPoint(x: 50.7962, y: 47.9972))
                        p.addLine(to: CGPoint(x: 6.17796, y: 47.9972))
                        p.addCurve(to: CGPoint(x: 0, y: 41.2408), control1: CGPoint(x: 2.16172, y: 47.9972), control2: CGPoint(x: 0, y: 45.6318))
                        p.closeSubpath()
                        return p
                    }(),
                    {
                        let p = CGMutablePath()
                        p.move(to: CGPoint(x: 58.7892, y: 39.8362))
                        p.addLine(to: CGPoint(x: 79.685, y: 3.36589))
                        p.addCurve(to: CGPoint(x: 89.1435, y: 3.36589), control1: CGPoint(x: 82.2553, y: -1.11494), control2: CGPoint(x: 86.5647, y: -1.12899))
                        p.addLine(to: CGPoint(x: 110.031, y: 39.8362))
                        p.addCurve(to: CGPoint(x: 105.555, y: 48), control1: CGPoint(x: 112.77, y: 44.6148), control2: CGPoint(x: 111.082, y: 48))
                        p.addLine(to: CGPoint(x: 63.2649, y: 48))
                        p.addCurve(to: CGPoint(x: 58.7892, y: 39.8362), control1: CGPoint(x: 57.7408, y: 48), control2: CGPoint(x: 56.0526, y: 44.6204))
                        p.closeSubpath()
                        return p
                    }(),
                    {
                        let p = CGMutablePath()
                        p.move(to: CGPoint(x: 137.257, y: 39.4064))
                        p.addLine(to: CGPoint(x: 137.257, y: 41.2408))
                        p.addCurve(to: CGPoint(x: 130.478, y: 47.9972), control1: CGPoint(x: 137.257, y: 44.9716), control2: CGPoint(x: 134.221, y: 47.9972))
                        p.addLine(to: CGPoint(x: 121.49, y: 47.9972))
                        p.addCurve(to: CGPoint(x: 114.712, y: 41.2408), control1: CGPoint(x: 117.745, y: 47.9972), control2: CGPoint(x: 114.712, y: 44.9716))
                        p.addLine(to: CGPoint(x: 114.712, y: 7.56014))
                        p.addCurve(to: CGPoint(x: 121.493, y: 0.800986), control1: CGPoint(x: 114.712, y: 3.82658), control2: CGPoint(x: 117.748, y: 0.800986))
                        p.addLine(to: CGPoint(x: 136.831, y: 0.800986))
                        p.addCurve(to: CGPoint(x: 160.965, y: 20.1318), control1: CGPoint(x: 154.652, y: 0.800986), control2: CGPoint(x: 160.965, y: 8.65577))
                        p.addCurve(to: CGPoint(x: 137.259, y: 39.4064), control1: CGPoint(x: 160.965, y: 31.6077), control2: CGPoint(x: 154.753, y: 39.2827))
                        p.addLine(to: CGPoint(x: 137.257, y: 39.4064))
                        p.closeSubpath()
                        return p
                    }(),
                    {
                        let p = CGMutablePath()
                        p.move(to: CGPoint(x: 164.191, y: 41.2408))
                        p.addLine(to: CGPoint(x: 164.191, y: 7.56014))
                        p.addCurve(to: CGPoint(x: 170.972, y: 0.800986), control1: CGPoint(x: 164.191, y: 3.82658), control2: CGPoint(x: 167.227, y: 0.800986))
                        p.addLine(to: CGPoint(x: 179.96, y: 0.800986))
                        p.addCurve(to: CGPoint(x: 186.739, y: 7.55733), control1: CGPoint(x: 183.706, y: 0.800986), control2: CGPoint(x: 186.739, y: 3.82658))
                        p.addLine(to: CGPoint(x: 186.739, y: 16.5331))
                        p.addLine(to: CGPoint(x: 195.743, y: 16.5331))
                        p.addCurve(to: CGPoint(x: 202.522, y: 23.2894), control1: CGPoint(x: 199.489, y: 16.5331), control2: CGPoint(x: 202.522, y: 19.5587))
                        p.addLine(to: CGPoint(x: 202.522, y: 41.238))
                        p.addCurve(to: CGPoint(x: 195.743, y: 47.9944), control1: CGPoint(x: 202.522, y: 44.9688), control2: CGPoint(x: 199.486, y: 47.9944))
                        p.addLine(to: CGPoint(x: 170.972, y: 47.9944))
                        p.addCurve(to: CGPoint(x: 164.194, y: 41.238), control1: CGPoint(x: 167.227, y: 47.9944), control2: CGPoint(x: 164.194, y: 44.9688))
                        p.addLine(to: CGPoint(x: 164.191, y: 41.2408))
                        p.closeSubpath()
                        return p
                    }(),
                    {
                        let p = CGMutablePath()
                        p.move(to: CGPoint(x: 240.304, y: 16.5331))
                        p.addLine(to: CGPoint(x: 230.6, y: 16.5331))
                        p.addCurve(to: CGPoint(x: 236.386, y: 24.1884), control1: CGPoint(x: 233.943, y: 17.4854), control2: CGPoint(x: 236.386, y: 20.5532))
                        p.addCurve(to: CGPoint(x: 230.05, y: 31.9842), control1: CGPoint(x: 236.386, y: 28.0259), control2: CGPoint(x: 233.669, y: 31.2257))
                        p.addLine(to: CGPoint(x: 240.321, y: 31.9842))
                        p.addCurve(to: CGPoint(x: 247.082, y: 38.7237), control1: CGPoint(x: 244.712, y: 31.9842), control2: CGPoint(x: 247.082, y: 34.3412))
                        p.addLine(to: CGPoint(x: 247.082, y: 41.2549))
                        p.addCurve(to: CGPoint(x: 240.315, y: 47.9972), control1: CGPoint(x: 247.082, y: 45.6346), control2: CGPoint(x: 244.712, y: 47.9972))
                        p.addLine(to: CGPoint(x: 212.982, y: 47.9972))
                        p.addCurve(to: CGPoint(x: 206.215, y: 41.2549), control1: CGPoint(x: 208.582, y: 47.9972), control2: CGPoint(x: 206.215, y: 45.6346))
                        p.addLine(to: CGPoint(x: 206.215, y: 7.5433))
                        p.addCurve(to: CGPoint(x: 212.982, y: 0.800986), control1: CGPoint(x: 206.215, y: 3.1608), control2: CGPoint(x: 208.582, y: 0.800986))
                        p.addLine(to: CGPoint(x: 240.315, y: 0.800986))
                        p.addCurve(to: CGPoint(x: 247.082, y: 7.5433), control1: CGPoint(x: 244.712, y: 0.800986), control2: CGPoint(x: 247.082, y: 3.1608))
                        p.addLine(to: CGPoint(x: 247.082, y: 9.77668))
                        p.addCurve(to: CGPoint(x: 240.306, y: 16.5302), control1: CGPoint(x: 247.082, y: 13.5074), control2: CGPoint(x: 244.047, y: 16.5302))
                        p.addLine(to: CGPoint(x: 240.304, y: 16.5331))
                        p.closeSubpath()
                        return p
                    }(),
                ]

                for cgPath in paths {
                    let transformed = cgPath.copy(using: [transform])!
                    let path = Path(transformed)
                    context.fill(path, with: .color(color))
                }
            }
        }
        .frame(width: height * aspectRatio, height: height)
    }
}
