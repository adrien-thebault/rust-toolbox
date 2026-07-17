/// sort data
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub enum Sort {
    /// sorted request
    Sorted {
        /// direction of each field (by string name) we want to sort
        items: Vec<(String, SortDirection)>,
    },

    /// unsorted request
    #[default]
    Unsorted,
}

/// sort direction
#[derive(Clone, Default, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SortDirection {
    /// ascendant sort
    #[default]
    Asc,

    /// descendant sort
    Desc,
}
