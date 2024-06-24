use crate::Allocator;
use log::trace;
use space_time::allocator::{ArrayAccessor, ArrayAccessorMut};
use space_time::errors::InvalidIdError;
use space_time::{SnapshotId, SpaceTime};
use std::cmp::Ordering;
use std::fmt::{Debug, Formatter};

/// Trait for types that can be simulated by [`Simulator`].
///
/// Note that [`Simulator`] only requires [`Simulatable<SimulationAllocator>`] to be implemented.
pub trait Simulatable<A: Allocator>: Debug {
    /// Advance the simulation one tick. This operation should be deterministic.
    ///
    /// The `tick` operation is deterministic if the state of `allocator` after calling `tick` only
    /// depends on the state of `allocator` before calling `tick`.
    /// (Assuming no methods that take `&mut self` are called).
    fn tick(&self, allocator: &mut A);

    /// Drop this simulatable cleanly by first removing its state from `allocator`.
    ///
    /// It is not required to call this before [`Drop::drop`]ing this simulatable. However, if you
    /// don't, the only way to clean up the memory allocated by this simulatable in `allocator` is
    /// to drop the `allocator` entirely (since the ids of the objects allocated by this simulatable
    /// will no longer be available to only remove those objects).
    fn drop(self, allocator: &mut A);
}

/// The [`Allocator`] used by a [`Simulator`].
///
/// Not named `Allocator` to avoid conflicts with [`crate::Allocator`].
#[derive(Debug)]
pub struct SimulationAllocator(SpaceTime);

impl Allocator for SimulationAllocator {
    type Id<T> = <SpaceTime as Allocator>::Id<T>;
    type ArrayId<T> = <SpaceTime as Allocator>::ArrayId<T>;

    #[inline]
    fn insert<T: Clone + 'static>(&mut self, object: T) -> Self::Id<T> {
        self.0.insert(object)
    }

    #[inline]
    fn insert_array<T: Copy + 'static>(&mut self, object: T, n: usize) -> Self::ArrayId<T> {
        self.0.insert_array(object, n)
    }

    #[inline]
    fn remove<T: Clone + 'static>(&mut self, id: Self::Id<T>) -> Result<(), InvalidIdError> {
        self.0.remove(id)
    }

    #[inline]
    fn remove_array<T: Copy + 'static>(
        &mut self,
        id: Self::ArrayId<T>,
    ) -> Result<(), InvalidIdError> {
        self.0.remove_array(id)
    }

    #[inline]
    fn pop<T: Clone + 'static>(&mut self, id: Self::Id<T>) -> Result<T, InvalidIdError> {
        self.0.pop(id)
    }

    #[inline]
    fn get<T: Clone + 'static>(&self, id: Self::Id<T>) -> Result<&T, InvalidIdError> {
        self.0.get(id)
    }

    #[inline]
    fn get_array<T: Copy + 'static>(
        &self,
        id: Self::ArrayId<T>,
    ) -> Result<impl ArrayAccessor<T>, InvalidIdError> {
        self.0.get_array(id)
    }

    #[inline]
    fn get_mut<T: Clone + 'static>(&mut self, id: Self::Id<T>) -> Result<&mut T, InvalidIdError> {
        self.0.get_mut(id)
    }

    #[inline]
    fn get_array_mut<T: Copy + 'static>(
        &mut self,
        id: Self::ArrayId<T>,
    ) -> Result<impl ArrayAccessorMut<T>, InvalidIdError> {
        self.0.get_array_mut(id)
    }
}

/// A simulator can simulate any `Simulatable`.
/// It provides a full linear simulation history with undo and redo capabilities.
#[derive(Debug)]
pub struct Simulator<S: Simulatable<SimulationAllocator>> {
    allocator: SimulationAllocator,
    /// The object that's being simulated.
    ///
    /// The simulatable itself is provided externally (using a [`GenericSimulatable`]), but its
    /// allocator is provided by us ([`space_time`]).
    simulatable: S,
    /// Ordered timeline of `(head, snapshot_id)` pairs, where `snapshot_id` is an id in
    /// [`space_time`] of the snapshot taken at state `head.state_index`. A snapshot is made after
    /// construction of this `Simulator`, so this should never be empty. `head` holds the values
    /// [`head`] had right after the snapshot was made (so `head.base_snapshot_index` will be the
    /// index of this `(head, snapshot_id)` pair).
    ///
    /// The state indices of the snapshots (`head.state_index`) are guaranteed to be increasing
    /// strictly. This means there will never be two snapshots of the same state index.
    snapshots: Vec<(Head, SnapshotId)>,
    /// Ordered timeline of `(step_index, custom_tick)` pairs, where `custom_tick` is the
    /// [`IntoTick`] that was passed to [`step_with`] to use as custom tick function at step
    /// `step_index`.
    custom_ticks: Vec<(StepIndex, Tick<S>)>,
    head: Head,
}

impl<S: Simulatable<SimulationAllocator>> Simulator<S> {
    /// Create a new `Simulator` with a clear history and a [`Simulatable`] in reset state.
    ///
    /// `simulatable_constructor` must be a function that constructs the [`Simulatable`] that must
    /// be simulated based on the [`SimulationAllocator`] passed to it. Note that the constructed
    /// [`Simulatable`] must manage all its state through the provided [`SimulationAllocator`],
    /// otherwise simulation will not work correctly.
    pub fn new<F>(simulatable_constructor: F) -> Self
    where
        F: FnOnce(&mut SimulationAllocator) -> S,
    {
        let mut allocator = SimulationAllocator(SpaceTime::new());
        let simulatable = simulatable_constructor(&mut allocator);
        let state_index = StateIndex::new();
        let snapshot_id = allocator.0.make_snapshot();
        let head = Head {
            state_index,
            base_snapshot_index: 0,
            next_custom_tick_index: 0,
        };
        Self {
            allocator,
            simulatable,
            snapshots: vec![(head.clone(), snapshot_id)],
            custom_ticks: Vec::new(),
            head,
        }
    }

    /// Provides immutable access to the simulatable.
    ///
    /// Prefer this over [`inspect`](Self::inspect) if all you need is access to the simulatable's
    /// configuration, and not to its state.
    pub fn simulatable(&self) -> &S {
        &self.simulatable
    }

    /// Provides immutable access to the allocator that was used by [`Simulator::new`] to construct
    /// the simulatable.
    ///
    /// This is the same as the first element of the tuple returned by [`inspect`](Self::inspect).
    pub fn allocator(&self) -> &SimulationAllocator {
        &self.allocator
    }

    /// Returns an accessor that can be used to immutably inspect the simulatable's state.
    ///
    /// If you only need to inspect the simulatable's config (not its state), you can just use
    /// [`simulatable`](Self::simulatable).
    ///
    /// If you need to mutate the simulatable's state (such as to write registers, write memory,
    /// perform side effects, execute instructions, etc.), then you should use
    /// [`step_with`](Self::step_with).
    pub fn inspect(&self) -> (&SimulationAllocator, &S) {
        trace!("Inspecting simulatable");
        (&self.allocator, &self.simulatable)
    }

    /// Returns the number of steps from the start of history to the current state.
    pub fn current_steps(&self) -> usize {
        self.head.state_index.steps_since(StateIndex::new()).len()
    }

    /// Returns the total number of steps from the start of history to the last stored state.
    pub fn available_steps(&self) -> usize {
        self.head_at_last_state()
            .state_index
            .steps_since(StateIndex::new())
            .len()
    }

    /// Advance the simulation forward by one tick, but use a custom `tick` function instead of
    /// [`Simulatable::tick`].
    ///
    /// Note that this will erase the forward history, i.e. all future undone steps can no longer be
    /// redone hereafter.
    ///
    /// Note that every method on the board or any of its components may trigger side effects if it
    /// takes a `&mut impl Allocator` (if not documented otherwise).
    ///
    /// It is possible to use the default [`Simulatable::tick`] function as part of the provided
    /// custom tick function. However, it is not advisable to create a wrapper that regularly
    /// uses the default [`Simulatable::tick`], since that means in these cases calling
    /// [`step()`](Self::step) would suffice and be strongly recommended as it provides a more
    /// optimized implementation.
    ///
    /// For example:
    ///
    /// ```
    /// use red_planet_core::{Allocator, board::Board};
    /// use red_planet_core::simulator::{Simulator, Simulatable, SimulationAllocator};
    ///
    /// #[derive(Debug)]
    /// struct Component;
    /// impl Simulatable<SimulationAllocator> for Component {
    ///     fn tick(&self, allocator: &mut SimulationAllocator) { println!("tick") }
    ///     fn drop(self, allocator: &mut SimulationAllocator) { }
    /// }
    ///
    /// let mut simulator = Simulator::new(|allocator| Component);
    /// // In this example, this:
    /// simulator.step_with("bad tick", |allocator, comp| comp.tick(allocator));
    /// // would be better done using:
    /// simulator.step();
    /// ```
    pub fn step_with<F, R>(&mut self, name: &'static str, custom_tick: F) -> R
    where
        F: 'static + Fn(&mut SimulationAllocator, &S) -> R,
    {
        trace!("Stepping simulator once with custom step \"{name}\"");

        if self.is_head_detached() {
            trace!("Simulator HEAD is detached");
            self.clear_forward_history();
        }

        let res = custom_tick(&mut self.allocator, &self.simulatable);

        let tick = Tick {
            name,
            tick: Box::new(move |allocator, simulatable| {
                custom_tick(allocator, simulatable);
            }),
        };

        self.custom_ticks
            .push((self.head.state_index.next_step(), tick));

        self.head.state_index = self.head.state_index.next();

        if self.should_create_snapshot() {
            self.make_snapshot();
        }

        res
    }

    /// Advance the simulation forward by one tick.
    ///
    /// This will erase the forward history, i.e. all future undone steps can no longer be redone
    /// hereafter.
    pub fn step(&mut self) {
        trace!("Stepping simulator once");

        if self.is_head_detached() {
            trace!("Simulator HEAD is detached");
            self.clear_forward_history();
        }

        trace!("Ticking the simulatable");
        self.simulatable.tick(&mut self.allocator);

        self.head.state_index = self.head.state_index.next();

        if self.should_create_snapshot() {
            self.make_snapshot();
        }
    }

    /// Replay a previously undone step. Assumes such a step exists, and ignores any snapshots that
    /// may have been made.
    fn replay_step(&mut self) {
        trace!("Replaying previously undone step in simulator");

        let step_index = self.head.state_index.next_step();

        match self.custom_ticks.get(self.head.next_custom_tick_index) {
            Some((s, custom_tick)) if *s == step_index => {
                trace!("Step to replay used custom tick \"{}\"", &custom_tick.name);
                (custom_tick.tick)(&mut self.allocator, &self.simulatable);
            }
            _ => self.simulatable.tick(&mut self.allocator),
        }

        self.head.state_index = self.head.state_index.next();
    }

    /// Revert the simulation by one step. Returns `false` if there was nothing to undo.
    pub fn undo_step(&mut self) -> bool {
        // Determine target state
        let target_state_index = match self.head.state_index.previous() {
            None => {
                // Cannot undo when at the start of history
                trace!("Undoing step in simulator while at the start of history; doing nothing");
                return false;
            }
            Some(state_index) => state_index,
        };

        trace!("Undoing step in simulator");

        // If the current state is the newest AND dirty, then we need to save it, so it can be
        // restored later when redoing.
        if self.is_head_dirty() {
            self.make_snapshot();
        }

        self.go_to_state(target_state_index);

        true
    }

    /// Revert the simulation to the most recent past state for which `pred` returns `true`.
    /// Returns `false` if `pred` returned `false` for all states in the past, or if there are no
    /// steps to undo.
    ///
    /// Note that the "most recent past state" never includes the current state.
    ///
    /// The arguments to `pred` are the same as the elements returned by [`Self::inspect`].
    ///
    /// `pred` may be called on any step, even future ones. However, it is guaranteed that `pred`
    /// will be called *at most once* per step. Additionally, it will have been called at least
    /// `n` times when `n` steps are eventually undone.
    /// But, **the order in which the steps are visited is not defined**.
    ///
    /// This method also takes a `visit` function that will be applied in reverse chronological
    /// order to arbitrary intermediate states. It will at least be applied to the most recent state
    /// for which `pred` returns `true`; which will also always be the last call to `visit`, since
    /// `visit` is called in reverse chronological order. Note that `visit` will be called between
    /// 1 and `n` times when `n` steps are eventually undone.
    pub fn undo_steps_until(
        &mut self,
        pred: impl Fn(&SimulationAllocator, &S) -> bool,
        mut visit: impl FnMut(&Self),
    ) -> bool {
        if self.head.state_index.previous().is_none() {
            // Cannot undo when at the start of history
            trace!("Undoing steps in simulator while at the start of history; doing nothing");
            return false;
        }

        // If the current state is the newest AND dirty, then we need to save it, so it can be
        // restored later when redoing.
        if self.is_head_dirty() {
            self.make_snapshot();
        }

        // Upper bound (exclusive) on state indices to visit the first iteration.
        let mut top_state_index = self.head.state_index;

        let base_snapshot_index = match self.is_head_at_snapshot() {
            false => self.head.base_snapshot_index,
            true => self.head.base_snapshot_index - 1,
        };

        for snapshot_index in (0..=base_snapshot_index).rev() {
            self.go_to_snapshot(snapshot_index);

            // If we got in this loop, `top_state_index` must have a previous state.
            let end_state_index = top_state_index.previous().unwrap();

            let mut last_matched_state =
                pred(&self.allocator, &self.simulatable).then_some(self.head.state_index);

            for _ in end_state_index.steps_since(self.head.state_index) {
                self.replay_step();
                if pred(&self.allocator, &self.simulatable) {
                    last_matched_state = Some(self.head.state_index);
                }
            }

            if let Some(state_index) = last_matched_state {
                if state_index < self.head.state_index {
                    self.go_to_snapshot(snapshot_index);
                }
                while self.head.state_index != state_index {
                    self.replay_step();
                }
                visit(self);
                return true;
            }

            visit(self);

            // Upper bound (exclusive) on state indices to visit next iteration.
            top_state_index = self.snapshots[snapshot_index].0.state_index;
        }

        // Reached the start of history, while `pred` still hasn't returned `true`.
        self.go_to_snapshot(0);
        false
    }

    /// Redo the last undone step. Returns `false` if there was nothing to redo.
    pub fn redo_step(&mut self) -> bool {
        if !self.is_head_detached() {
            // Cannot redo when nothing has been undone
            trace!("Redoing step in simulator while at the end of history; doing nothing");
            return false;
        }

        trace!("Redoing step in simulator");

        self.go_to_state(self.head.state_index.next());

        true
    }

    /// Jump to the state resulting from applying `steps` steps from the start of history.
    pub fn go_to(&mut self, steps: usize) {
        // If the current state is the newest AND dirty, then we need to save it, so it can be
        // restored later when redoing.
        if self.is_head_dirty() {
            self.make_snapshot();
        }

        self.go_to_state(StateIndex::new().add_steps(steps));
    }

    /// Heuristic to determine if we should take a snapshot already.
    fn should_create_snapshot(&self) -> bool {
        trace!("Checking whether to create snapshot");
        // TODO: make a better heuristic
        self.steps_since_last_snapshot() > 2048
    }

    /// Returns the number of completed steps since last snapshot.
    fn steps_since_last_snapshot(&self) -> usize {
        self.head
            .state_index
            .steps_since(self.head_at_last_snapshot().state_index)
            .len()
    }

    /// Makes a new snapshot of the current state, updating `self.snapshots` and HEAD.
    fn make_snapshot(&mut self) {
        trace!("Making snapshot of simulator state");
        let snapshot_id = self.allocator.0.make_snapshot();
        self.head.base_snapshot_index = self.snapshots.len();
        // It is important that `self.head.base_snapshot_index` has been updated before `self.head`
        // is added to `self.snapshots`
        self.snapshots.push((self.head.clone(), snapshot_id));
    }

    /// Returns the state HEAD had right after the snapshot with the highest `state_index` was made.
    /// Note that the [`Head::base_snapshot_index`] will be equal to [`self.last_snapshot_index()`].
    fn head_at_last_snapshot(&self) -> &Head {
        &self.snapshots[self.last_snapshot_index()].0
    }

    /// Returns the state of HEAD that is the furthest of all stored history. This will be the HEAD
    /// at the last snapshot if a past state is checked-out, or the current HEAD if
    /// `self.is_head_dirty()`.
    fn head_at_last_state(&self) -> &Head {
        match self.is_head_detached() {
            true => self.head_at_last_snapshot(),
            false => &self.head,
        }
    }

    fn last_snapshot_index(&self) -> usize {
        self.snapshots
            .len()
            .checked_sub(1)
            .expect("the initial snapshot should always be present")
    }

    /// Returns `true` if HEAD does not represent the most recent state (i.e. if there is any
    /// forward history).
    ///
    /// If this returns `false`, then no more [`redo_step`]s are possible.
    fn is_head_detached(&self) -> bool {
        // At first sight this might seem incorrect for the following scenario:
        //  - HEAD is clean, currently the last snapshot
        //  - 3 new steps are simulated
        //  - undo() is called
        // But it is still correct then, because `undo` creates a new snapshot of the current state
        // before calling `go_to_state`. Every method to change state either creates a new snapshot
        // first, or mentions it "discards" the current state. Either way we only need to check
        // HEAD's base snapshot differs from the last snapshot.
        self.head.base_snapshot_index != self.last_snapshot_index()
    }

    /// Returns `true` if HEAD is currently at a clean checkout of a snapshot.
    ///
    /// If this is `false`, then [`Self::is_head_dirty()`] is implied.
    fn is_head_at_snapshot(&self) -> bool {
        self.snapshots[self.head.base_snapshot_index].0.state_index == self.head.state_index
    }

    /// Returns `true` if HEAD is at the end of history and dirty.
    ///
    /// The meaning of "dirty" here is to indicate that there are "unsaved" changes since the last
    /// snapshot (were "last" is the end of history). This implies checking out another snapshot
    /// would be destructive at this point.
    ///
    /// Note that this differs from the "dirty" of [`SpaceTime::head`], in the sense that
    /// [`SpaceTime::head`] will also be dirty if HEAD is in between two snapshots from the past.
    fn is_head_dirty(&self) -> bool {
        !self.is_head_detached() && !self.is_head_at_snapshot()
    }

    /// Discard the current state and revert to the state at `target_state_index`.
    ///
    /// Assumes `target_state_index` is reachable even after discarding the current state.
    fn go_to_state(&mut self, target_state_index: StateIndex) {
        trace!("Reverting to state {target_state_index:?}");

        if self.head.state_index == target_state_index {
            return;
        }

        // Determine the last snapshot still before the target state
        let target_base_snapshot_index = self.find_base_snapshot(target_state_index);

        if target_base_snapshot_index != self.head.base_snapshot_index
            || target_state_index < self.head.state_index
        {
            self.go_to_snapshot(target_base_snapshot_index);
        }

        while self.head.state_index != target_state_index {
            self.replay_step();
        }
    }

    /// Returns the index in `snapshots` of the last snapshot that's before or on `state`.
    fn find_base_snapshot(&self, state_index: StateIndex) -> usize {
        self.snapshots
            .partition_point(|(h, _)| h.state_index <= state_index)
            .checked_sub(1)
            .expect("every state comes on or after the initial snapshot")
    }

    /// Discard the current state and revert to the snapshot at `self.snapshots[snapshot_index]`.
    fn go_to_snapshot(&mut self, snapshot_index: usize) {
        trace!("Reverting to snapshot with index {snapshot_index}");

        let (Head { state_index, .. }, snapshot_id) = self.snapshots[snapshot_index];

        if self.head.state_index == state_index {
            return;
        }

        // Compute new indices in `custom_ticks`
        let next_custom_tick_index = self.custom_ticks.partition_point(|(s, _)| *s < state_index);

        self.allocator.0.checkout(snapshot_id).unwrap();

        self.head = Head {
            state_index,
            base_snapshot_index: snapshot_index,
            next_custom_tick_index,
        };
    }

    fn clear_forward_history(&mut self) {
        trace!("Clearing forward history of simulator");
        for (_, snapshot_id) in self.snapshots.drain((self.head.base_snapshot_index + 1)..) {
            self.allocator.0.drop_snapshot(snapshot_id).unwrap();
        }
        self.custom_ticks.truncate(self.head.next_custom_tick_index);
    }
}

struct Tick<S: Simulatable<SimulationAllocator>> {
    #[allow(dead_code)]
    name: &'static str,
    #[allow(clippy::type_complexity)]
    tick: Box<dyn Fn(&mut SimulationAllocator, &S) + 'static>,
}

impl<S: Simulatable<SimulationAllocator>> Debug for Tick<S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tick").finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct Head {
    /// The index of the current state in the linear timeline.
    /// The current [`step_index`] is derived from this.
    pub state_index: StateIndex,
    /// Index in [`Simulator::snapshots`] of the last snapshot before or on [`state_index`].
    pub base_snapshot_index: usize,
    /// Index in [`Simulator::custom_ticks`] of the next custom tick (starts at `0` if there are no
    /// custom ticks).
    pub next_custom_tick_index: usize,
}

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
struct StateIndex(usize);

impl StateIndex {
    pub fn new() -> Self {
        Self(0)
    }

    pub fn next(self) -> Self {
        self.add_steps(1)
    }

    pub fn previous(self) -> Option<Self> {
        self.0.checked_sub(1).map(Self)
    }

    pub fn next_step(self) -> StepIndex {
        StepIndex(self.0)
    }

    #[allow(unused)]
    pub fn previous_step(self) -> Option<StepIndex> {
        self.0.checked_sub(1).map(StepIndex)
    }

    pub fn add_steps(self, n: usize) -> Self {
        Self(
            self.0
                .checked_add(n)
                .expect("attempt to index more simulation states than fit in a usize"),
        )
    }

    pub fn steps_since(self, older_state: Self) -> impl ExactSizeIterator<Item = StepIndex> {
        (older_state.next_step().0..self.next_step().0).map(StepIndex)
    }
}

impl PartialEq<StepIndex> for StateIndex {
    /// A [`StateIndex`] and a [`StepIndex`] are never equal! This always returns `false`.
    fn eq(&self, _other: &StepIndex) -> bool {
        false
    }
}

impl PartialOrd<StepIndex> for StateIndex {
    fn partial_cmp(&self, other: &StepIndex) -> Option<Ordering> {
        // Compare the state index as even numbers (0, 2, 4, ...)
        // with the step index as odd numbers (1, 3, 5, ...)
        // such that the order becomes: state 0 -> step 0 -> state 1 -> step 1 -> ...
        self.0
            .wrapping_mul(2)
            .partial_cmp(&other.0.wrapping_mul(2).wrapping_add(1))
    }
}

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
struct StepIndex(usize);

impl StepIndex {
    #[allow(unused)]
    pub fn next(self) -> Self {
        Self(
            self.0
                .checked_add(1)
                .expect("attempt to index more simulation steps than fit in a usize"),
        )
    }

    #[allow(unused)]
    pub fn previous(self) -> Option<Self> {
        self.0.checked_sub(1).map(Self)
    }

    #[allow(unused)]
    pub fn state_after(self) -> StateIndex {
        StateIndex(
            self.0
                .checked_add(1)
                .expect("attempt to index more simulation states than fit in a usize"),
        )
    }

    #[allow(unused)]
    pub fn state_before(self) -> StateIndex {
        StateIndex(self.0)
    }
}

impl PartialEq<StateIndex> for StepIndex {
    /// A [`StepIndex`] and a [`StateIndex`] are never equal! This always returns `false`.
    fn eq(&self, _other: &StateIndex) -> bool {
        false
    }
}

impl PartialOrd<StateIndex> for StepIndex {
    fn partial_cmp(&self, other: &StateIndex) -> Option<Ordering> {
        // Compare the step index as odd numbers (1, 3, 5, ...)
        // with the state index as even numbers (0, 2, 4, ...)
        // such that the order becomes: state 0 -> step 0 -> state 1 -> step 1 -> ...
        self.0
            .wrapping_mul(2)
            .wrapping_add(1)
            .partial_cmp(&other.0.wrapping_mul(2))
    }
}

#[cfg(test)]
mod tests {
    use super::StateIndex;

    #[test]
    fn compare_state_and_step_index() {
        // State has 1 predecessor state, implying also 1 predecessor step
        let state0 = StateIndex::new();
        let state1 = state0.next();
        let step0 = state0.next_step();
        let step1 = step0.next();

        assert!(state0 < step0);
        assert!(step0 < state1);
        assert!(state1 < step1);

        assert!(step1 > state1);
        assert!(state1 > step0);
        assert!(step0 > state0);

        #[allow(clippy::nonminimal_bool)]
        {
            assert!(!(state0 > step0));
            assert!(!(state0 > step1));
            assert!(!(step0 > state1));
            assert!(!(state1 > step1));
        }

        assert_ne!(state0, step0);
        assert_ne!(state1, step0);
        assert_ne!(state1, step0);
        assert_ne!(state1, step1);
    }
}
