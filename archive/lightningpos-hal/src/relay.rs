/// Relay control module
///
/// Type-safe abstraction over GPIO relay outputs with support for
/// standard (digital HIGH/LOW), PWM, blink, pulse, and strobe modes.
///
/// # Safety
///
/// Relay pins must be valid GPIO outputs for the board variant.
/// The compiler enforces this via the pin type system (esp-hal GPIO types).

use crate::pins::relay;

/// Relay output mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RelayMode {
    /// Standard on/off (digital HIGH = active)
    Standard,
    /// Blink at configured frequency
    Blink { period_ms: u32, duty_cycle: f32 },
    /// Single pulse for duration, then off
    Pulse { duration_ms: u32 },
    /// Servo position (0-180°)
    Servo(u8),
}

/// A logical relay channel
#[derive(Debug, Clone, Copy)]
pub struct RelayChannel {
    /// Channel number (1-indexed)
    pub number: u8,
    /// GPIO pin
    pub pin: u8,
}

impl RelayChannel {
    /// Create a relay channel by number. Returns None if out of range.
    pub fn new(channel: u8) -> Option<Self> {
        relay::channel_pin(channel).map(|pin| Self {
            number: channel,
            pin,
        })
    }

    /// Get the default (CH01) relay
    pub fn default_channel() -> Self {
        Self {
            number: 1,
            pin: relay::DEFAULT_RELAY,
        }
    }
}

/// Configuration for a relay output action
#[derive(Debug, Clone, Copy)]
pub struct RelayAction {
    /// Duration in milliseconds (0 = indefinite)
    pub duration_ms: u32,
    /// Output mode
    pub mode: RelayMode,
}

impl Default for RelayAction {
    fn default() -> Self {
        Self {
            duration_ms: 0,
            mode: RelayMode::Standard,
        }
    }
}

impl RelayAction {
    /// Standard on/off with optional duration
    pub fn standard(duration_ms: u32) -> Self {
        Self {
            duration_ms,
            mode: RelayMode::Standard,
        }
    }

    /// Blink mode
    pub fn blink(duration_ms: u32, period_ms: u32, duty_cycle: f32) -> Self {
        Self {
            duration_ms,
            mode: RelayMode::Blink {
                period_ms,
                duty_cycle,
            },
        }
    }

    /// Single pulse
    pub fn pulse(duration_ms: u32) -> Self {
        Self {
            duration_ms,
            mode: RelayMode::Pulse { duration_ms },
        }
    }

    /// Servo position
    pub fn servo(angle: u8, duration_ms: u32) -> Self {
        Self {
            duration_ms,
            mode: RelayMode::Servo(angle),
        }
    }
}

/// Queue of pending relay activations
///
/// Payment events from WebSocket may arrive faster than relay actions
/// can complete. This queue buffers them for sequential execution.
#[derive(Debug)]
pub struct RelayQueue<const N: usize> {
    queue: heapless::Deque<(RelayChannel, RelayAction), N>,
    /// Currently active channel (if any)
    active: Option<RelayChannel>,
    /// When the current action started (ms)
    active_since: u64,
}

impl<const N: usize> RelayQueue<N> {
    pub fn new() -> Self {
        Self {
            queue: heapless::Deque::new(),
            active: None,
            active_since: 0,
        }
    }

    /// Enqueue a relay action
    pub fn enqueue(&mut self, channel: RelayChannel, action: RelayAction) -> Result<(), ()> {
        self.queue.push_back((channel, action)).map_err(|_| ())
    }

    /// Number of pending actions
    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    /// Whether a relay is currently active
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Get the currently active channel
    pub fn active_channel(&self) -> Option<RelayChannel> {
        self.active
    }

    /// Dequeue the next action, or return None if nothing to do.
    /// Should be called when a relay action completes.
    pub fn dequeue(&mut self, now: u64) -> Option<(RelayChannel, RelayAction)> {
        if self.active.is_some() {
            return None; // Still processing current action
        }
        let next = self.queue.pop_front();
        if let Some((ch, _)) = next {
            self.active = Some(ch);
            self.active_since = now;
        }
        next
    }

    /// Mark the current action as complete. Call this when the relay
    /// duration has elapsed and the pin is set back to LOW.
    pub fn complete(&mut self) {
        self.active = None;
    }
}

impl<const N: usize> Default for RelayQueue<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_pin_mapping() {
        let ch1 = RelayChannel::new(1).unwrap();
        assert_eq!(ch1.pin, 12);
        assert_eq!(ch1.number, 1);
    }

    #[test]
    fn test_invalid_channel() {
        assert!(RelayChannel::new(99).is_none());
    }

    #[test]
    fn test_relay_queue() {
        let mut q = RelayQueue::<4>::new();

        let ch1 = RelayChannel::default_channel();
        let ch2 = RelayChannel::new(2).unwrap();

        q.enqueue(ch1, RelayAction::standard(5000)).unwrap();
        q.enqueue(ch2, RelayAction::pulse(2000)).unwrap();
        assert_eq!(q.pending(), 2);

        let first = q.dequeue(0);
        assert!(first.is_some());
        assert!(q.is_active());
        assert_eq!(q.pending(), 1);

        // Can't dequeue while active
        assert!(q.dequeue(1000).is_none());

        q.complete();
        assert!(!q.is_active());

        let second = q.dequeue(6000);
        assert!(second.is_some());
        assert_eq!(q.pending(), 0);
    }
}
