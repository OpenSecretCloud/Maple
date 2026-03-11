use gtk::cairo;
use gtk::prelude::*;
use gtk4 as gtk;

use crate::theme::Color;

pub fn full_wordmark(color: Color, height: i32) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(((height as f64) * (248.0 / 48.0)).round() as i32);
    area.set_content_height(height);
    area.set_draw_func(move |_, cr: &cairo::Context, _, height| {
        cr.scale(height as f64 / 48.0, height as f64 / 48.0);
        color.set_source(cr);
        draw_full_wordmark(cr);
        let _ = cr.fill();
    });
    area
}

pub fn abbr_wordmark(color: Color, height: i32) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(((height as f64) * (100.0 / 32.0)).round() as i32);
    area.set_content_height(height);
    area.set_draw_func(move |_, cr: &cairo::Context, _, height| {
        cr.scale(height as f64 / 32.0, height as f64 / 32.0);
        color.set_source(cr);
        draw_abbr_wordmark(cr);
        let _ = cr.fill();
    });
    area
}

fn draw_abbr_wordmark(cr: &cairo::Context) {
    cr.new_path();
    cr.move_to(0.0, 27.4798);
    cr.line_to(0.0, 4.02964);
    cr.curve_to(0.0, 0.420626, 3.85183, -1.16383, 6.79613, 0.948761);
    cr.line_to(17.9439, 10.3599);
    cr.line_to(29.0916, 0.948761);
    cr.curve_to(32.0378, -1.16383, 35.8878, 0.420626, 35.8878, 4.02964);
    cr.line_to(35.8878, 27.4798);
    cr.curve_to(35.8878, 29.967, 33.8642, 31.984, 31.3689, 31.984);
    cr.line_to(4.11864, 31.984);
    cr.curve_to(1.44115, 31.984, 0.0, 30.4071, 0.0, 27.4798);
    cr.close_path();

    cr.move_to(55.3789, 26.0034);
    cr.line_to(55.3789, 27.2264);
    cr.curve_to(55.3789, 29.7135, 53.3553, 31.7306, 50.8601, 31.7306);
    cr.line_to(44.8681, 31.7306);
    cr.curve_to(42.371, 31.7306, 40.3493, 29.7135, 40.3493, 27.2264);
    cr.line_to(40.3493, 4.77258);
    cr.curve_to(40.3493, 2.28354, 42.3729, 0.266478, 44.87, 0.266478);
    cr.line_to(55.0952, 0.266478);
    cr.curve_to(66.9758, 0.266478, 71.1846, 5.503, 71.1846, 13.1537);
    cr.curve_to(71.1846, 20.8043, 67.0434, 25.921, 55.3808, 26.0034);
    cr.close_path();

    cr.move_to(74.3353, 27.2264);
    cr.line_to(74.3353, 4.77258);
    cr.curve_to(74.3353, 2.28354, 76.3589, 0.266478, 78.8561, 0.266478);
    cr.line_to(84.848, 0.266478);
    cr.curve_to(87.3451, 0.266478, 89.3669, 2.28354, 89.3669, 4.77071);
    cr.line_to(89.3669, 10.7545);
    cr.line_to(95.3701, 10.7545);
    cr.curve_to(97.8672, 10.7545, 99.8889, 12.7716, 99.8889, 15.2588);
    cr.line_to(99.8889, 27.2245);
    cr.curve_to(99.8889, 29.7117, 97.8653, 31.7288, 95.3701, 31.7288);
    cr.line_to(78.8561, 31.7288);
    cr.curve_to(76.3589, 31.7288, 74.3372, 29.7117, 74.3372, 27.2245);
    cr.close_path();
}

fn draw_full_wordmark(cr: &cairo::Context) {
    cr.new_path();
    cr.move_to(0.0, 41.2408);
    cr.line_to(0.0, 6.06562);
    cr.curve_to(0.0, 0.652098, 5.77774, -1.72459, 10.1942, 1.4443);
    cr.line_to(26.9158, 15.561);
    cr.line_to(43.6375, 1.4443);
    cr.curve_to(48.0567, -1.72459, 53.8317, 0.652098, 53.8317, 6.06562);
    cr.line_to(53.8317, 41.2408);
    cr.curve_to(53.8317, 44.9716, 50.7962, 47.9972, 47.0534, 47.9972);
    cr.line_to(6.17796, 47.9972);
    cr.curve_to(2.16172, 47.9972, 0.0, 45.6318, 0.0, 41.2408);
    cr.close_path();

    cr.move_to(58.7892, 39.8362);
    cr.line_to(79.685, 3.36589);
    cr.curve_to(82.2553, -1.11494, 86.5647, -1.12899, 89.1435, 3.36589);
    cr.line_to(110.031, 39.8362);
    cr.curve_to(112.77, 44.6148, 111.082, 48.0, 105.555, 48.0);
    cr.line_to(63.2649, 48.0);
    cr.curve_to(57.7408, 48.0, 56.0526, 44.6204, 58.7892, 39.8362);
    cr.close_path();

    cr.move_to(137.257, 39.4064);
    cr.line_to(137.257, 41.2408);
    cr.curve_to(137.257, 44.9716, 134.221, 47.9972, 130.478, 47.9972);
    cr.line_to(121.49, 47.9972);
    cr.curve_to(117.745, 47.9972, 114.712, 44.9716, 114.712, 41.2408);
    cr.line_to(114.712, 7.56014);
    cr.curve_to(114.712, 3.82658, 117.748, 0.800986, 121.493, 0.800986);
    cr.line_to(136.831, 0.800986);
    cr.curve_to(154.652, 0.800986, 160.965, 8.65577, 160.965, 20.1318);
    cr.curve_to(160.965, 31.6077, 154.753, 39.2827, 137.259, 39.4064);
    cr.close_path();

    cr.move_to(164.191, 41.2408);
    cr.line_to(164.191, 7.56014);
    cr.curve_to(164.191, 3.82658, 167.227, 0.800986, 170.972, 0.800986);
    cr.line_to(179.96, 0.800986);
    cr.curve_to(183.706, 0.800986, 186.739, 3.82658, 186.739, 7.55733);
    cr.line_to(186.739, 16.5331);
    cr.line_to(195.743, 16.5331);
    cr.curve_to(199.489, 16.5331, 202.522, 19.5587, 202.522, 23.2894);
    cr.line_to(202.522, 41.238);
    cr.curve_to(202.522, 44.9688, 199.486, 47.9944, 195.743, 47.9944);
    cr.line_to(170.972, 47.9944);
    cr.curve_to(167.227, 47.9944, 164.194, 44.9688, 164.194, 41.238);
    cr.close_path();

    cr.move_to(240.304, 16.5331);
    cr.line_to(230.6, 16.5331);
    cr.curve_to(233.943, 17.4854, 236.386, 20.5532, 236.386, 24.1884);
    cr.curve_to(236.386, 28.0259, 233.669, 31.2257, 230.05, 31.9842);
    cr.line_to(240.321, 31.9842);
    cr.curve_to(244.712, 31.9842, 247.082, 34.3412, 247.082, 38.7237);
    cr.line_to(247.082, 41.2549);
    cr.curve_to(247.082, 45.6346, 244.712, 47.9972, 240.315, 47.9972);
    cr.line_to(212.982, 47.9972);
    cr.curve_to(208.582, 47.9972, 206.215, 45.6346, 206.215, 41.2549);
    cr.line_to(206.215, 7.5433);
    cr.curve_to(206.215, 3.1608, 208.582, 0.800986, 212.982, 0.800986);
    cr.line_to(240.315, 0.800986);
    cr.curve_to(244.712, 0.800986, 247.082, 3.1608, 247.082, 7.5433);
    cr.line_to(247.082, 9.77668);
    cr.curve_to(247.082, 13.5074, 244.047, 16.5302, 240.306, 16.5302);
    cr.close_path();
}
