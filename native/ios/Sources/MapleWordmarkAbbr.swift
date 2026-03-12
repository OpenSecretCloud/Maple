import SwiftUI

struct MapleWordmarkAbbr: View {
    var color: Color = .pebble50
    var height: CGFloat = 20

    private let aspectRatio: CGFloat = 100.0 / 32.0

    var body: some View {
        GeometryReader { _ in
            Canvas { context, size in
                let scale = size.height / 32.0
                let transform = CGAffineTransform(scaleX: scale, y: scale)

                let paths: [CGPath] = [
                    // M
                    {
                        let p = CGMutablePath()
                        p.move(to: CGPoint(x: 0, y: 27.4798))
                        p.addLine(to: CGPoint(x: 0, y: 4.02964))
                        p.addCurve(to: CGPoint(x: 6.79613, y: 0.948761), control1: CGPoint(x: 0, y: 0.420626), control2: CGPoint(x: 3.85183, y: -1.16383))
                        p.addLine(to: CGPoint(x: 17.9439, y: 10.3599))
                        p.addLine(to: CGPoint(x: 29.0916, y: 0.948761))
                        p.addCurve(to: CGPoint(x: 35.8878, y: 4.02964), control1: CGPoint(x: 32.0378, y: -1.16383), control2: CGPoint(x: 35.8878, y: 0.420626))
                        p.addLine(to: CGPoint(x: 35.8878, y: 27.4798))
                        p.addCurve(to: CGPoint(x: 31.3689, y: 31.984), control1: CGPoint(x: 35.8878, y: 29.967), control2: CGPoint(x: 33.8642, y: 31.984))
                        p.addLine(to: CGPoint(x: 4.11864, y: 31.984))
                        p.addCurve(to: CGPoint(x: 0, y: 27.4798), control1: CGPoint(x: 1.44115, y: 31.984), control2: CGPoint(x: 0, y: 30.4071))
                        p.closeSubpath()
                        return p
                    }(),
                    // P
                    {
                        let p = CGMutablePath()
                        p.move(to: CGPoint(x: 55.3789, y: 26.0034))
                        p.addLine(to: CGPoint(x: 55.3789, y: 27.2264))
                        p.addCurve(to: CGPoint(x: 50.8601, y: 31.7306), control1: CGPoint(x: 55.3789, y: 29.7135), control2: CGPoint(x: 53.3553, y: 31.7306))
                        p.addLine(to: CGPoint(x: 44.8681, y: 31.7306))
                        p.addCurve(to: CGPoint(x: 40.3493, y: 27.2264), control1: CGPoint(x: 42.371, y: 31.7306), control2: CGPoint(x: 40.3493, y: 29.7135))
                        p.addLine(to: CGPoint(x: 40.3493, y: 4.77258))
                        p.addCurve(to: CGPoint(x: 44.87, y: 0.266478), control1: CGPoint(x: 40.3493, y: 2.28354), control2: CGPoint(x: 42.3729, y: 0.266478))
                        p.addLine(to: CGPoint(x: 55.0952, y: 0.266478))
                        p.addCurve(to: CGPoint(x: 71.1846, y: 13.1537), control1: CGPoint(x: 66.9758, y: 0.266478), control2: CGPoint(x: 71.1846, y: 5.503))
                        p.addCurve(to: CGPoint(x: 55.3808, y: 26.0034), control1: CGPoint(x: 71.1846, y: 20.8043), control2: CGPoint(x: 67.0434, y: 25.921))
                        p.addLine(to: CGPoint(x: 55.3789, y: 26.0034))
                        p.closeSubpath()
                        return p
                    }(),
                    // L
                    {
                        let p = CGMutablePath()
                        p.move(to: CGPoint(x: 74.3353, y: 27.2264))
                        p.addLine(to: CGPoint(x: 74.3353, y: 4.77258))
                        p.addCurve(to: CGPoint(x: 78.8561, y: 0.266478), control1: CGPoint(x: 74.3353, y: 2.28354), control2: CGPoint(x: 76.3589, y: 0.266478))
                        p.addLine(to: CGPoint(x: 84.848, y: 0.266478))
                        p.addCurve(to: CGPoint(x: 89.3669, y: 4.77071), control1: CGPoint(x: 87.3451, y: 0.266478), control2: CGPoint(x: 89.3669, y: 2.28354))
                        p.addLine(to: CGPoint(x: 89.3669, y: 10.7545))
                        p.addLine(to: CGPoint(x: 95.3701, y: 10.7545))
                        p.addCurve(to: CGPoint(x: 99.8889, y: 15.2588), control1: CGPoint(x: 97.8672, y: 10.7545), control2: CGPoint(x: 99.8889, y: 12.7716))
                        p.addLine(to: CGPoint(x: 99.8889, y: 27.2245))
                        p.addCurve(to: CGPoint(x: 95.3701, y: 31.7288), control1: CGPoint(x: 99.8889, y: 29.7117), control2: CGPoint(x: 97.8653, y: 31.7288))
                        p.addLine(to: CGPoint(x: 78.8561, y: 31.7288))
                        p.addCurve(to: CGPoint(x: 74.3372, y: 27.2245), control1: CGPoint(x: 76.3589, y: 31.7288), control2: CGPoint(x: 74.3372, y: 29.7117))
                        p.addLine(to: CGPoint(x: 74.3353, y: 27.2264))
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
