use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex};
use crate::CutSegment;

use mpeg2ts_reader::{demultiplex, packet, psi, packet_filter_switch};

// Custom packet filter switcher for mpeg2ts_reader
packet_filter_switch! {
    Scte35FilterSwitch<Scte35DemuxContext> {
        Pat: demultiplex::PatPacketFilter<Scte35DemuxContext>,
        Pmt: demultiplex::PmtPacketFilter<Scte35DemuxContext>,
        Nul: demultiplex::NullPacketFilter<Scte35DemuxContext>,
        Scte35: psi::SectionPacketConsumer<
            psi::CompactSyntaxSectionProcessor<
                psi::BufferCompactSyntaxParser<
                    scte35_reader::Scte35SectionProcessor<Scte35Processor, Scte35DemuxContext>
                >
            >
        >,
    }
}

// Custom demux context to pass collected events down to the SCTE-35 processor
pub struct Scte35DemuxContext {
    changeset: demultiplex::FilterChangeset<Scte35FilterSwitch>,
    events: Arc<Mutex<Vec<Scte35Event>>>,
}

impl Scte35DemuxContext {
    fn new(events: Arc<Mutex<Vec<Scte35Event>>>) -> Self {
        Self {
            changeset: demultiplex::FilterChangeset::default(),
            events,
        }
    }
}

impl demultiplex::DemuxContext for Scte35DemuxContext {
    type F = Scte35FilterSwitch;

    fn filter_changeset(&mut self) -> &mut demultiplex::FilterChangeset<Self::F> {
        &mut self.changeset
    }

    fn construct(&mut self, req: demultiplex::FilterRequest<'_, '_>) -> Self::F {
        match req {
            demultiplex::FilterRequest::ByPid(packet::Pid::PAT) => {
                Scte35FilterSwitch::Pat(demultiplex::PatPacketFilter::default())
            }
            demultiplex::FilterRequest::Pmt { pid, program_number } => {
                Scte35FilterSwitch::Pmt(demultiplex::PmtPacketFilter::new(pid, program_number))
            }
            demultiplex::FilterRequest::ByStream { stream_type, .. } => {
                if stream_type == scte35_reader::SCTE35_STREAM_TYPE {
                    let processor = Scte35Processor {
                        events: self.events.clone(),
                    };
                    let section_processor = scte35_reader::Scte35SectionProcessor::new(processor);
                    let buffer_parser = psi::BufferCompactSyntaxParser::new(section_processor);
                    let compact_processor = psi::CompactSyntaxSectionProcessor::new(buffer_parser);
                    Scte35FilterSwitch::Scte35(psi::SectionPacketConsumer::new(compact_processor))
                } else {
                    Scte35FilterSwitch::Nul(demultiplex::NullPacketFilter::default())
                }
            }
            _ => Scte35FilterSwitch::Nul(demultiplex::NullPacketFilter::default()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Scte35Event {
    Start { pts: u64, duration: Option<u64> },
    End { pts: u64 },
}

pub struct Scte35Processor {
    events: Arc<Mutex<Vec<Scte35Event>>>,
}

// Extract the raw duration field value from a SpliceDuration struct.
// Since the fields of SpliceDuration are private in scte35-reader, we use Debug serialization.
fn get_splice_duration(dur: &scte35_reader::SpliceDuration) -> Option<u64> {
    let s = format!("{:?}", dur);
    if let Some(pos) = s.find("duration:") {
        let sub = &s[pos + 9..];
        let num_str: String = sub.chars().filter(|c| c.is_ascii_digit()).collect();
        num_str.parse().ok()
    } else {
        None
    }
}

// Adjust PTS based on the header's pts_adjustment
fn get_pts(splice_time: &scte35_reader::SpliceTime, pts_adjustment: u64) -> Option<u64> {
    match splice_time {
        scte35_reader::SpliceTime::Timed(Some(pts)) => {
            Some((*pts + pts_adjustment) & 0x1_FFFF_FFFF)
        }
        _ => None,
    }
}

impl scte35_reader::SpliceInfoProcessor for Scte35Processor {
    fn process(
        &self,
        header: scte35_reader::SpliceInfoHeader<'_>,
        command: scte35_reader::SpliceCommand,
        descriptors: scte35_reader::SpliceDescriptors<'_>,
    ) {
        let pts_adj = header.pts_adjustment();
        let mut events = self.events.lock().unwrap();

        match command {
            scte35_reader::SpliceCommand::SpliceInsert { splice_detail, .. } => {
                if let scte35_reader::SpliceInsert::Insert { network_indicator, splice_mode, duration, .. } = splice_detail {
                    let pts = match splice_mode {
                        scte35_reader::SpliceMode::Program(splice_time) => get_pts(&splice_time, pts_adj),
                        _ => None,
                    };
                    if let Some(p) = pts {
                        let dur = duration.as_ref().and_then(get_splice_duration);
                        match network_indicator {
                            scte35_reader::NetworkIndicator::Out => {
                                events.push(Scte35Event::Start {
                                    pts: p,
                                    duration: dur,
                                });
                            }
                            scte35_reader::NetworkIndicator::In => {
                                events.push(Scte35Event::End { pts: p });
                            }
                        }
                    }
                }
            }
            scte35_reader::SpliceCommand::TimeSignal { splice_time } => {
                if let Some(pts) = get_pts(&splice_time, pts_adj) {
                    for descriptor in &descriptors {
                        if let Ok(scte35_reader::SpliceDescriptor::SegmentationDescriptor { descriptor_detail, .. }) = descriptor {
                            match descriptor_detail {
                                scte35_reader::SegmentationDescriptor::Insert { segmentation_type_id, segmentation_duration, .. } => {
                                    match segmentation_type_id {
                                        scte35_reader::SegmentationTypeId::ProviderAdvertisementStart |
                                        scte35_reader::SegmentationTypeId::DistributorAdvertisementStart |
                                        scte35_reader::SegmentationTypeId::ProviderPlacementOpportunityStart |
                                        scte35_reader::SegmentationTypeId::DistributorPlacementOpportunityStart |
                                        scte35_reader::SegmentationTypeId::BreakStart => {
                                            events.push(Scte35Event::Start {
                                                pts,
                                                duration: segmentation_duration,
                                            });
                                        }
                                        scte35_reader::SegmentationTypeId::ProviderAdvertisementEnd |
                                        scte35_reader::SegmentationTypeId::DistributorAdvertisementEnd |
                                        scte35_reader::SegmentationTypeId::ProviderPlacementOpportunityEnd |
                                        scte35_reader::SegmentationTypeId::DistributorPlacementOpportunityEnd |
                                        scte35_reader::SegmentationTypeId::BreakEnd => {
                                            events.push(Scte35Event::End { pts });
                                        }
                                        _ => {}
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

// Internal pure helper to process chronological events and build CutSegments
fn process_events(mut events: Vec<Scte35Event>) -> Vec<CutSegment> {
    if events.is_empty() {
        return Vec::new();
    }

    // Sort events by PTS
    events.sort_by_key(|e| match e {
        Scte35Event::Start { pts, .. } => *pts,
        Scte35Event::End { pts } => *pts,
    });

    let mut segments = Vec::new();
    let mut current_start: Option<u64> = None;

    for event in events.iter() {
        match event {
            Scte35Event::Start { pts, duration } => {
                if let Some(dur) = duration {
                    let start_t = *pts as f64 / 90000.0;
                    let end_t = (*pts + *dur) as f64 / 90000.0;
                    segments.push(CutSegment {
                        start_seconds: start_t,
                        end_seconds: end_t,
                        start_frame: 0,
                        end_frame: 0,
                        confidence: 1.0,
                    });
                } else {
                    current_start = Some(*pts);
                }
            }
            Scte35Event::End { pts } => {
                if let Some(start_pts) = current_start {
                    if *pts > start_pts {
                        let start_t = start_pts as f64 / 90000.0;
                        let end_t = *pts as f64 / 90000.0;
                        segments.push(CutSegment {
                            start_seconds: start_t,
                            end_seconds: end_t,
                            start_frame: 0,
                            end_frame: 0,
                            confidence: 1.0,
                        });
                    }
                    current_start = None;
                }
            }
        }
    }
    segments
}

pub fn parse_scte35<P: AsRef<Path>>(path: P) -> Result<Option<Vec<CutSegment>>, anyhow::Error> {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return Ok(None),
    };

    let events = Arc::new(Mutex::new(Vec::new()));
    let mut ctx = Scte35DemuxContext::new(events.clone());
    let mut demux = demultiplex::Demultiplex::new(&mut ctx);

    let mut buf = vec![0u8; 188 * 1000];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        demux.push(&mut ctx, &buf[..n]);
    }

    let events_guard = events.lock().unwrap();
    let segments = process_events(events_guard.clone());

    if segments.is_empty() {
        Ok(None)
    } else {
        Ok(Some(segments))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_events_duration() {
        let events = vec![
            Scte35Event::Start {
                pts: 90000,
                duration: Some(2700000), // 30 seconds
            }
        ];
        let segments = process_events(events);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].start_seconds, 1.0);
        assert_eq!(segments[0].end_seconds, 31.0);
    }

    #[test]
    fn test_process_events_start_end_matching() {
        let events = vec![
            Scte35Event::Start {
                pts: 90000,
                duration: None,
            },
            Scte35Event::End {
                pts: 2790000, // 31 seconds
            }
        ];
        let segments = process_events(events);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].start_seconds, 1.0);
        assert_eq!(segments[0].end_seconds, 31.0);
    }

    #[test]
    fn test_process_events_multiple() {
        let events = vec![
            Scte35Event::Start {
                pts: 90000,
                duration: Some(900000), // 10 seconds (1s -> 11s)
            },
            Scte35Event::Start {
                pts: 1800000, // 20 seconds
                duration: None,
            },
            Scte35Event::End {
                pts: 2700000, // 30 seconds (20s -> 30s)
            }
        ];
        let segments = process_events(events);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].start_seconds, 1.0);
        assert_eq!(segments[0].end_seconds, 11.0);
        assert_eq!(segments[1].start_seconds, 20.0);
        assert_eq!(segments[1].end_seconds, 30.0);
    }

    #[test]
    fn test_get_pts() {
        let time = scte35_reader::SpliceTime::Timed(Some(1000));
        assert_eq!(get_pts(&time, 500), Some(1500));
        assert_eq!(get_pts(&time, 0x1_FFFF_FFFF), Some(999)); // 33-bit wrapping
    }
}
