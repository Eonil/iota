use rustbox::{Color, RustBox};

use buffer::{Line, Buffer};
use cursor::Direction;
use cursor::Cursor;
use input::Input;
use uibuf::UIBuffer;

use utils;

pub struct CursorGuard<'a> {
    linenum: &'a mut uint,
    offset: &'a mut uint,
    cursor: Cursor<'a>,
}

impl<'a> Deref<Cursor<'a>> for CursorGuard<'a> {
    fn deref(&self) -> &Cursor<'a> {
        &self.cursor
    }
}

impl<'a> DerefMut<Cursor<'a>> for CursorGuard<'a> {
    fn deref_mut(&mut self) -> &mut Cursor<'a> {
        &mut self.cursor
    }
}

#[unsafe_destructor]
impl<'a> Drop for CursorGuard<'a> {
    fn drop(&mut self) {
        // Update line number and offset
        *self.linenum = self.cursor.get_linenum();
        *self.offset = self.cursor.get_actual_offset();
    }
}

/// A View is an abstract Window (into a Buffer).
///
/// It draws a portion of a Buffer to a UIBuffer which in turn is drawn to the
/// screen. It maintains the status bar for the current view, the "dirty status"
/// which is whether the buffer has been modified or not and a number of other
/// pieces of information.
pub struct View<'v> {
    pub buffer: Buffer,
    // the line number of the topmost `Line` for the View to render
    pub top_line_num: uint,
    pub linenum: uint,
    pub offset: uint,

    uibuf: UIBuffer,
    threshold: int,
}

impl<'v> View<'v> {
    pub fn new(source: Input, rb: &RustBox) -> View<'v> {
        let buffer = match source {
            Input::Filename(path) => {
                match path {
                    Some(s) => Buffer::new_from_file(Path::new(s)),
                    None    => Buffer::new(),
                }
            },
            Input::Stdin(reader) => {
                Buffer::new_from_reader(reader)
            },
        };

        let height: uint = utils::get_term_height(rb);
        let width: uint = utils::get_term_width(rb);

        // NOTE(greg): this may not play well with resizing
        let uibuf = UIBuffer::new(width, height);

        View {
            buffer: buffer,
            top_line_num: 0,
            uibuf: uibuf,
            threshold: 5,
            linenum: 0,
            offset: 0,
        }
    }

    pub fn cursor<'b>(&'b mut self) -> CursorGuard<'b> {
        let View {ref mut buffer, ref mut offset, ref mut linenum, .. } = *self;
        let cursor = Cursor::new(&mut buffer.lines[*linenum], *offset);
        CursorGuard { cursor: cursor, offset: offset, linenum: linenum }
    }

    /// Clear the buffer
    ///
    /// Fills every cell in the UIBuffer with the space (' ') char.
    pub fn clear(&mut self, rb: &RustBox) {
        self.uibuf.fill(' ');
        self.uibuf.draw_everything(rb);
    }

    pub fn get_height(&self) -> uint {
        // NOTE(greg): when the status bar needs to move up, this value should be changed
        self.uibuf.get_height() -1
    }

    pub fn get_width(&self) -> uint {
        self.uibuf.get_width()
    }

    pub fn draw(&mut self, rb: &RustBox) {
        let end_line = self.get_height();

        let mut line_num = 0;
        let mut offset = 0;

        for (total_offset, ch) in self.buffer.text.iter().enumerate() {
            if total_offset == self.buffer.cursor {
                utils::draw_cursor(rb, offset, line_num-self.top_line_num);
            }
            if line_num >= self.top_line_num {
                if line_num < end_line {
                    if *ch == '\n' {
                        offset = 0;
                        line_num += 1;
                    } else {
                        let index = line_num - self.top_line_num;
                        offset = draw_char(&mut self.uibuf, ch, offset, index, self.top_line_num);
                    }
                }
            }
        }

        self.uibuf.draw_everything(rb);
    }

    pub fn draw_status(&mut self, rb: &RustBox) {
        let buffer_status = self.buffer.get_status_text();
        let status_text = format!("{}", buffer_status).into_bytes();
        let status_text_len = status_text.len();
        let width = self.get_width();
        let height = self.get_height();


        for index in range(0, width) {
            let mut ch: char = ' ';
            if index < status_text_len {
                ch = status_text[index] as char;
            }
            self.uibuf.update_cell(index, height, ch, Color::Black, Color::Blue);
        }

        self.uibuf.draw_range(rb, height, height+1);
    }

    pub fn resize(&mut self, rb: &RustBox) {
        let width = self.uibuf.get_width();
        self.clear(rb);
        self.uibuf = UIBuffer::new(width, 15);
    }

    pub fn move_cursor(&mut self, direction: Direction) {
        match direction {
            // FIXME: move this to the editor module
            Direction::Up    => { self.buffer.shift_cursor(Direction::Up); },
            Direction::Down  => { self.buffer.shift_cursor(Direction::Down); },
            Direction::Right => { self.buffer.shift_cursor(Direction::Right); },
            Direction::Left  => { self.buffer.shift_cursor(Direction::Left); },
        }
    }

    pub fn move_cursor_to_line_end(&mut self) {
        self.cursor().set_offset(::std::uint::MAX);
    }

    pub fn move_cursor_to_line_start(&mut self) {
        self.cursor().set_offset(0);
    }

    fn move_cursor_right(&mut self) {
        // let cursor_offset = self.cursor().get_visible_offset();
        // let next_offset = cursor_offset + 1;
        let width = self.get_width() - 1;

        let cursor_offset = self.buffer.get_cursor_screen_offset();
        let next_offset = cursor_offset + 1;
        if next_offset < width {
            // self.cursor().move_right()
            self.buffer.shift_cursor(Direction::Right);
        }
    }

    // TODO(greg): refactor this method with move_cursor_down
    pub fn move_cursor_up(&mut self) {
        let cursor_linenum = self.cursor().get_linenum();

        if cursor_linenum == 0 { return }
        let prev_linenum = cursor_linenum - 1;

        let num_lines = self.buffer.lines.len() - 1;
        if prev_linenum > num_lines { return }

        self.set_cursor_line(prev_linenum);

        let cursor_linenum = self.cursor().get_linenum() as int;
        let cursor_offset = cursor_linenum - self.top_line_num as int;

        if cursor_offset < self.threshold {
            let times = cursor_offset - self.threshold;
            self.move_top_line_n_times(times);
        }

    }

    // TODO(greg): refactor this method with move_cursor_up
    pub fn move_cursor_down(&mut self) {
        let cursor_linenum = self.cursor().get_linenum();
        let next_linenum = cursor_linenum + 1;

        let num_lines = self.buffer.lines.len() - 1;
        if next_linenum > num_lines { return }

        self.set_cursor_line(next_linenum);

        let cursor_linenum = self.cursor().get_linenum() as int;
        let cursor_offset = cursor_linenum - self.top_line_num as int;
        let height = self.get_height() as int;

        if cursor_offset >= (height - self.threshold) {
            let times = cursor_offset - (height - self.threshold) + 1;
            self.move_top_line_n_times(times);
        }
    }

    fn set_cursor_line<'b>(&'b mut self, linenum: uint) {
        let vis_width = self.cursor().get_visible_offset();
        let mut offset = 0;
        let mut vis_acc = 0;
        let line = &*self.buffer.lines[linenum].data;
        for _ in line.char_indices().take_while(|&(i, c)| {
            offset = line.char_range_at(i).next;
            vis_acc += ::utils::char_width(c, false, 4, vis_acc).unwrap_or(0);
            vis_acc < vis_width
        }) {}
        if self.offset == 0 { offset = 0 }
        self.offset = offset;
        self.linenum = linenum;
    }

    fn move_top_line_n_times(&mut self, mut num_times: int) {
        if num_times == 0 { return }

        // moving down
        if num_times > 0 {
            while num_times > 0 {
                self.top_line_num += 1;
                num_times -= 1;
            }
            return
        }

        // moving up
        if num_times < 0 {
            while num_times < 0 {
                if self.top_line_num == 0 { return }
                self.top_line_num -= 1;
                num_times += 1;
            }
            return
        }
    }

    pub fn delete_char(&mut self, direction: Direction) {
        // let (offset, line_num) = self.cursor().get_position();

        // if offset == 0 && direction.is_left() {
        //     // Must move the cursor up first so we aren't pointing at dangling memory
        //     self.move_cursor_up();
        //     let offset = self.buffer.join_line_with_previous(offset, line_num);
        //     self.cursor().set_offset(offset);
        //     return
        // }

        // let line_len = self.cursor().get_line_length();
        // if offset == line_len && direction.is_right() {
        //     // try to join the next line with the current line
        //     // if there is no next line, nothing will happen
        //     self.buffer.join_line_with_previous(offset, line_num+1);
        //     return
        // }

        // match direction {
        //     Direction::Left  => self.cursor().delete_backward_char(),
        //     Direction::Right => self.cursor().delete_forward_char(),
        //     _                => {}
        // }
        self.buffer.delete_char(direction);
    }

    pub fn insert_tab(&mut self) {
        // A tab is just 4 spaces
        for _ in range(0i, 4) {
            self.insert_char(' ');
        }
    }

    pub fn insert_char(&mut self, ch: char) {
        self.buffer.insert_char(ch);
    }
}

pub fn draw_char(buf: &mut UIBuffer, ch: &char, mut offset: uint, index: uint, top_line_num: uint) -> uint{
    let width = buf.get_width() -1;
    if offset < width {
        match *ch {
            '\t' => {
                let w = 4 - offset%4;
                for _ in range(0, w) {
                    buf.update_cell_content(offset, index, ' ');
                    offset += 1;
                }
            }
            _ => {
                // draw the character
                buf.update_cell_content(offset, index, *ch);
                offset += ch.width(false).unwrap_or(1);
            }
        }
    }

    // if the line is longer than the width of the view, draw a special char
    if offset == width {
        buf.update_cell_content(offset, index, '→');
    }

    offset
}

#[cfg(test)]
mod tests {

    use buffer::{Line, Buffer};
    use cursor::{Direction};
    use view::View;
    use uibuf::UIBuffer;
    use utils::data_from_str;

    fn setup_view<'v>() -> View<'v> {
        let mut view = View {
            buffer: Buffer::new(),
            top_line_num: 0,
            linenum: 0,
            offset: 0,
            uibuf: UIBuffer::new(50, 50),
            threshold: 5,
        };

        let first_line = Line::new(data_from_str("test"), 0);
        let second_line = Line::new(data_from_str("second"), 1);

        view.buffer.lines = vec!(first_line, second_line);
        view.set_cursor_line(0);

        return view
    }

    #[test]
    fn test_move_cursor_down() {
        let mut view = setup_view();
        view.move_cursor_down();

        assert_eq!(view.cursor().get_linenum(), 1);
        assert_eq!(view.cursor().get_line().data, data_from_str("second"));
    }

    #[test]
    fn test_move_cursor_up() {
        let mut view = setup_view();
        view.move_cursor_down();
        view.move_cursor_up();
        assert_eq!(view.cursor().get_linenum(), 0);
        assert_eq!(view.cursor().get_line().data, data_from_str("test"));
    }

    #[test]
    fn test_insert_line() {
        let mut view = setup_view();
        view.cursor().move_right();
        view.insert_line();

        assert_eq!(view.buffer.lines.len(), 3);
        assert_eq!(view.cursor().get_offset(), 0);
        assert_eq!(view.cursor().get_line().linenum, 1);
    }

    #[test]
    fn test_insert_char() {
        let mut view = setup_view();
        view.insert_char('t');

        assert_eq!(view.cursor().get_line().data, data_from_str("ttest"));
    }

    #[test]
    fn test_delete_char_to_right() {
        let mut view = setup_view();
        view.delete_char(Direction::Right);

        assert_eq!(view.cursor().get_line().data, data_from_str("est"));
    }

    #[test]
    fn test_delete_char_to_left() {
        let mut view = setup_view();
        view.cursor().move_right();
        view.delete_char(Direction::Left);

        assert_eq!(view.cursor().get_line().data, data_from_str("est"));
    }

    #[test]
    fn test_delete_char_at_start_of_line() {
        let mut view = setup_view();
        view.move_cursor_down();
        view.delete_char(Direction::Left);

        assert_eq!(view.cursor().get_line().data, data_from_str("testsecond"));
    }

    #[test]
    fn test_delete_char_at_end_of_line() {
        let mut view = setup_view();
        view.offset = 4;
        view.delete_char(Direction::Right);

        assert_eq!(view.cursor().get_line().data, data_from_str("testsecond"));
    }

    #[test]
    fn delete_char_when_line_is_empty_does_nothing() {
        let mut view = setup_view();
        view.buffer.lines = vec!(Line::new(String::new(), 0));
        view.linenum = 0;
        view.delete_char(Direction::Right);
        assert_eq!(view.cursor().get_line().data, data_from_str(""));
    }

    #[test]
    fn deleting_backward_at_start_of_first_line_does_nothing() {
        let mut view = setup_view();
        view.delete_char(Direction::Left);

        assert_eq!(view.buffer.lines.len(), 2);
        assert_eq!(view.cursor().get_line().data, data_from_str("test"));
    }
}
