// The song file (doc 18 §3) → a Tracktion Edit. The studio owns the song; the engine only builds what it says.
// Time: the Edit's time 0 is the song's bar 1 (the studio adds the song's offsetSec to reach video time). Ticks are
// 960 per quarter note, so a tick is 1/960 beat. A channel with no instrument chosen plays Tracktion's 4OSC for now;
// its Strudel sound comes with doc 28 M13 (v48).
#pragma once
#include <JuceHeader.h>
#include "TestPlugins.h"

namespace mfx
{
    namespace te = tracktion;

    inline te::BeatPosition beatsOf (const juce::var& ticks) { return te::BeatPosition::fromBeats ((double) ticks / 960.0); }

    // Each track's volume plugin keeps its own pan law and reads as linear when unset: always the -3 dB law here.
    inline void setTrackPanLaw (te::AudioTrack& t) { t.getVolumePlugin()->setPanLaw (te::PanLaw3dBCenter); }

    struct BuiltSong
    {
        std::unique_ptr<te::Edit> edit;
        int tracks = 0, clips = 0, notes = 0;
        double seconds = 0;
    };

    inline const juce::var* findById (const juce::var& list, const juce::String& id)
    {
        if (auto* a = list.getArray())
            for (auto& v : *a)
                if (v["id"].toString() == id) return &v;
        return nullptr;
    }

    inline void defaultInstrument (te::AudioTrack& track, bool drums)
    {
        auto& edit = track.edit;
        auto plugin = edit.getPluginCache().createNewPlugin (te::FourOscPlugin::xmlTypeName, {});
        track.pluginList.insertPlugin (plugin, 0, nullptr);
        if (auto* osc = dynamic_cast<te::FourOscPlugin*> (plugin.get()))
        {
            if (drums)
            {
                osc->ampSustain->setParameter (0.0f, juce::dontSendNotification);
                osc->ampDecay->setParameter (0.12f, juce::dontSendNotification);
                osc->ampRelease->setParameter (0.05f, juce::dontSendNotification);
            }
        }
    }

    inline BuiltSong buildSong (te::Engine& engine, const juce::var& song)
    {
        BuiltSong out;
        out.edit = te::Edit::createSingleTrackEdit (engine, te::Edit::EditRole::forEditing);
        auto& edit = *out.edit;
        // the master's level (doc 22: the mixer's master volume, 0 dB unless the song says otherwise)
        edit.getMasterVolumePlugin()->setVolumeDb ((float) (double) song["mixer"]["master"].getProperty ("volDb", 0.0));

        // tempo map and meter (the first point at tick 0; jumps or linear ramps after it)
        const juce::var tempoList = song["tempo"], meterList = song["meter"], channelList = song["channels"], itemList = song["playlist"]["items"];
        if (auto* tempo = tempoList.getArray())
        {
            for (int i = 0; i < tempo->size(); ++i)
            {
                const auto& p = tempo->getReference (i);
                const double bpm = (double) p["bpm"];
                if (i == 0) edit.tempoSequence.getTempo (0)->setBpm (bpm);
                else edit.tempoSequence.insertTempo (beatsOf (p["tick"]), bpm, p["shape"].toString() == "linear" ? 1.0f : 0.0f);
            }
        }
        if (auto* meter = meterList.getArray(); meter != nullptr && ! meter->isEmpty())
        {
            auto ts = edit.tempoSequence.getTimeSig (0);
            ts->numerator = (int) (*meter)[0]["num"];
            ts->denominator = (int) (*meter)[0]["den"];
        }

        // one track per channel, in the song's order
        auto* channels = channelList.getArray();
        const int n = channels ? channels->size() : 0;
        edit.ensureNumberOfAudioTracks (juce::jmax (1, n));
        auto tracks = te::getAudioTracks (edit);
        std::map<juce::String, te::AudioTrack*> trackOf;
        for (int i = 0; i < n; ++i)
        {
            const auto& c = channels->getReference (i);
            auto* t = tracks[i];
            t->setName (c["name"].toString());
            setTrackPanLaw (*t);
            t->getVolumePlugin()->setVolumeDb ((float) (double) c.getProperty ("volDb", 0.0));
            t->getVolumePlugin()->setPan ((float) (double) c.getProperty ("pan", 0.0));
            t->setMute ((bool) c.getProperty ("mute", false));
            t->setSolo ((bool) c.getProperty ("solo", false));
            if (! c["instrument"].isObject() || ! insertBuiltinInstrument (*t, c["instrument"]))
                defaultInstrument (*t, c["kind"].toString() == "drums");
            trackOf[c["id"].toString()] = t;
            ++out.tracks;
        }

        // the playlist: each pattern item puts a MIDI clip per channel at its tick
        if (auto* items = itemList.getArray())
        {
            for (auto& it : *items)
            {
                if (it["kind"].toString() != "pattern") continue;
                auto* pat = findById (song["patterns"], it["ref"].toString());
                if (pat == nullptr) continue;
                const auto at = beatsOf (it["at"]["tick"]);
                const double lenBeats = (double) it["len"] / 960.0;
                const juce::var notesVar = (*pat)["notes"];
                auto* notes = notesVar.getDynamicObject();
                if (notes == nullptr) continue;
                for (auto& prop : notes->getProperties())
                {
                    auto found = trackOf.find (prop.name.toString());
                    if (found == trackOf.end()) continue;
                    auto* list = prop.value.getArray();
                    if (list == nullptr || list->isEmpty()) continue;
                    // a note ringing past the section's end is kept whole: the clip runs to its end
                    double endBeats = lenBeats;
                    for (auto& nt : *list) endBeats = juce::jmax (endBeats, ((double) nt["t"] + (double) nt["len"]) / 960.0);
                    const auto t0 = edit.tempoSequence.toTime (at);
                    const auto t1 = edit.tempoSequence.toTime (at + te::BeatDuration::fromBeats (endBeats));
                    auto clip = found->second->insertMIDIClip ((*pat)["id"].toString(), te::TimeRange (t0, t1), nullptr);
                    if (clip == nullptr) continue;
                    auto& seq = clip->getSequence();
                    for (auto& nt : *list)
                    {
                        const int key = (int) nt["key"];
                        const double vel = (double) nt.getProperty ("vel", 0.8);
                        seq.addNote (key, beatsOf (nt["t"]), te::BeatDuration::fromBeats ((double) nt["len"] / 960.0),
                                     juce::jlimit (1, 127, (int) std::lround (vel * 127.0)), 0, nullptr);
                        ++out.notes;
                    }
                    ++out.clips;
                    out.seconds = juce::jmax (out.seconds, t1.inSeconds());
                }
            }
        }
        return out;
    }
}
