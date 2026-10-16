// Our built-in test instruments (doc 15 §4 timing tests): they make signals whose every sample the tests can compute on
// their own. A channel names one in the song as { "kind": "builtin", "name": "mfx-click" }.
//   mfx-click   one sample at the note's velocity (vel / 127) on both sides, on the note-on's own sample; nothing else.
#pragma once
#include <JuceHeader.h>

namespace mfx
{
    namespace te = tracktion;

    class ClickPlugin : public te::Plugin
    {
    public:
        explicit ClickPlugin (te::PluginCreationInfo info) : te::Plugin (info) {}
        ~ClickPlugin() override { notifyListenersOfDeletion(); }

        static const char* getPluginName() { return "mfx-click"; }
        static inline const char* xmlTypeName = "mfx-click";
        juce::String getName() const override { return getPluginName(); }
        juce::String getPluginType() override { return xmlTypeName; }
        juce::String getShortName (int) override { return getName(); }
        juce::String getSelectableDescription() override { return getName(); }
        bool isSynth() override { return true; }
        bool takesMidiInput() override { return true; }
        bool producesAudioWhenNoAudioInput() override { return true; }
        int getNumOutputChannelsGivenInputs (int) override { return 2; }
        BusLayout getBusses() const override { return BusLayout::singleStereoInOut(); }

        void initialise (const te::PluginInitialisationInfo&) override { carried.clear(); }
        void deinitialise() override {}
        void reset() override { carried.clear(); }

        void applyToBuffer (const te::PluginRenderContext& fc) override
        {
            if (fc.destBuffer == nullptr) return;
            const double rate = sampleRate;
            const int n = fc.bufferNumSamples;
            fc.destBuffer->clear (fc.bufferStartSample, n);
            // The block's first sample on the song's sample grid; each event lands at its own sample, and one that
            // rounds to this block's end is carried into the next block (never pulled back onto this block's last
            // sample, which would move it by one depending on the block size).
            const juce::int64 blockStart = std::llround (fc.editTime.getStart().inSeconds() * rate);
            auto put = [&] (juce::int64 at, float v)
            {
                const auto i = at - blockStart;
                if (i < 0 || i >= n) { if (i >= n) carried.push_back ({ at, v }); return; }
                for (int c = 0; c < fc.destBuffer->getNumChannels(); ++c)
                    fc.destBuffer->addSample (c, fc.bufferStartSample + (int) i, v);
            };
            auto waiting = std::move (carried);
            carried.clear();
            for (auto& [at, v] : waiting) put (at, v);
            // The note's own time on the song's sample grid, rounded once; a note exactly on a half sample always goes to
            // the later one (+1e-6 sample, far under any real difference), so float error in the block's start can't
            // flip it with the block size.
            const double start = fc.editTime.getStart().inSeconds();
            if (fc.bufferForMidiMessages != nullptr)
                for (auto& m : *fc.bufferForMidiMessages)
                    if (m.isNoteOn())
                        put (std::llround ((start + m.getTimeStamp()) * rate + 1.0e-6), m.getVelocity() / 127.0f);
        }

    private:
        std::vector<std::pair<juce::int64, float>> carried;
    };

    // Called once per te::Engine, before any song is built.
    inline void registerOurPlugins (te::Engine& engine) { engine.getPluginManager().createBuiltInType<ClickPlugin>(); }

    // A channel's { "kind": "builtin", "name": … } → that plugin as its instrument (true), or false.
    inline bool insertBuiltinInstrument (te::AudioTrack& track, const juce::var& entry)
    {
        if (entry["kind"].toString() != "builtin") return false;
        auto plugin = track.edit.getPluginCache().createNewPlugin (entry["name"].toString(), {});
        if (plugin == nullptr) return false;
        track.pluginList.insertPlugin (plugin, 0, nullptr);
        return true;
    }
}
