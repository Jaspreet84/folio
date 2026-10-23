//! Column sizing for tables, modeled on the browser "auto" table layout.

/// A column's minimum width is its longest word, capped so that one long URL
/// can't force a very wide column. Longer words get broken.
pub const MIN_CAP: usize = 20;

/// When even the minimum widths don't fit, columns shrink down to this.
pub const FLOOR: usize = 6;

/// Choose a width for each column, given each column's minimum (longest
/// word) and maximum (whole cell on one line), so that the total is at most
/// `avail`. Returns `None` when the table can't fit even with every column
/// at the floor width; the caller then shows rows as records instead.
pub fn column_widths(min: &[usize], max: &[usize], avail: usize) -> Option<Vec<usize>> {
    let sum_max: usize = max.iter().sum();
    if sum_max <= avail {
        return Some(max.to_vec());
    }
    let sum_min: usize = min.iter().sum();
    if sum_min <= avail {
        return Some(distribute(min, max, avail));
    }
    // Shrinking below the longest words: columns with short words keep them
    // whole first (a broken "Default" reads badly), and the columns with the
    // longest words (often URLs) give up the space.
    let mut w: Vec<usize> = min.iter().map(|&m| m.min(FLOOR)).collect();
    let used: usize = w.iter().sum();
    if used > avail {
        return None;
    }
    let mut left = avail - used;
    let mut order: Vec<usize> = (0..min.len()).collect();
    order.sort_by_key(|&i| min[i]);
    for i in order {
        let add = (min[i] - w[i]).min(left);
        w[i] += add;
        left -= add;
    }
    Some(w)
}

/// Start every column at `base` and share the remaining space in proportion
/// to how far each column is from its `cap`. Never exceeds a column's cap.
fn distribute(base: &[usize], cap: &[usize], avail: usize) -> Vec<usize> {
    let mut w = base.to_vec();
    let used: usize = w.iter().sum();
    let extra = avail.saturating_sub(used);
    let want: Vec<usize> = base.iter().zip(cap).map(|(b, c)| c.saturating_sub(*b)).collect();
    let total_want: usize = want.iter().sum();
    if total_want == 0 {
        return w;
    }
    let extra = extra.min(total_want);
    for (i, wi) in w.iter_mut().enumerate() {
        *wi += extra * want[i] / total_want;
    }
    // Rounding leaves a few columns over; give them to whoever is furthest
    // from its cap.
    let mut left = extra - (w.iter().sum::<usize>() - used);
    while left > 0 {
        let Some(i) = (0..w.len())
            .filter(|&i| w[i] < cap[i])
            .max_by_key(|&i| (cap[i] - w[i], usize::MAX - i))
        else {
            break;
        };
        w[i] += 1;
        left -= 1;
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_natural_widths_when_they_fit() {
        assert_eq!(column_widths(&[3, 5], &[10, 20], 40), Some(vec![10, 20]));
    }

    #[test]
    fn shares_space_by_need() {
        // Column 0 wants 10 more, column 1 wants 30 more; 20 to share.
        let w = column_widths(&[5, 5], &[15, 35], 30).unwrap();
        assert_eq!(w.iter().sum::<usize>(), 30);
        assert_eq!(w, vec![10, 20]);
    }

    #[test]
    fn shrinks_below_longest_word_when_needed() {
        let w = column_widths(&[20, 20, 20], &[40, 40, 40], 30).unwrap();
        assert_eq!(w.iter().sum::<usize>(), 30);
        assert!(w.iter().all(|&c| c >= FLOOR));
    }

    #[test]
    fn short_words_stay_whole_when_shrinking() {
        // "Default" (7) and "--paging" (10) fit whole; the URL column shrinks.
        assert_eq!(column_widths(&[10, 7, 20], &[10, 7, 100], 30), Some(vec![10, 7, 13]));
    }

    #[test]
    fn gives_up_when_floors_do_not_fit() {
        assert_eq!(column_widths(&[10; 8], &[20; 8], 40), None);
    }

    #[test]
    fn narrow_columns_keep_their_small_minimum() {
        // A column whose longest word is 2 never needs more than 2.
        let w = column_widths(&[2, 30], &[2, 60], 20).unwrap();
        assert_eq!(w[0], 2);
        assert_eq!(w.iter().sum::<usize>(), 20);
    }
}
