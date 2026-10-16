// Our built-in plugins. A song names one as { "kind": "builtin", "name": "mfx-…", …its settings }.
//   mfx-click   test instrument (doc 15 §4 timing tests): one sample at the note's velocity (vel / 127) on both sides,
//               on the note-on's own sample; nothing else, so the tests can compute every sample on their own.
//   mfx-delay   a delay in whole samples that reports its own latency, separately ("delaySamples", "reportSamples",
//               "gain"): track delay (doc 22 §4: + delays the track, − reports latency so PDC delays all the others)
//               and the delay-compensation tests (doc 15 Q7: a plugin that delays 512 and says so).
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

    class DelayPlugin : public te::Plugin
    {
    public:
        explicit DelayPlugin (te::PluginCreationInfo info) : te::Plugin (info) {}
        ~DelayPlugin() override { notifyListenersOfDeletion(); }

        static const char* getPluginName() { return "mfx-delay"; }
        static inline const char* xmlTypeName = "mfx-delay";
        juce::String getName() const override { return getPluginName(); }
        juce::String getPluginType() override { return xmlTypeName; }
        juce::String getShortName (int) override { return getName(); }
        juce::String getSelectableDescription() override { return getName(); }
        BusLayout getBusses() const override { return BusLayout::singleStereoInOut(); }
        int getNumOutputChannelsGivenInputs (int) override { return 2; }
        double getLatencySeconds() override { return sampleRate > 0 ? reportSamples / sampleRate : 0.0; }

        // set once, before the Edit plays (the song is built again for any change)
        int delaySamples = 0, reportSamples = 0;
        float gain = 1.0f;

        void initialise (const te::PluginInitialisationInfo&) override { reset(); }
        void deinitialise() override {}
        void reset() override
        {
            line.setSize (2, juce::jmax (1, delaySamples + 1));
            line.clear();
            pos = 0;
        }

        void applyToBuffer (const te::PluginRenderContext& fc) override
        {
            if (fc.destBuffer == nullptr) return;
            auto& b = *fc.destBuffer;
            const int chans = juce::jmin (2, b.getNumChannels());
            if (delaySamples > 0)
            {
                const int size = line.getNumSamples();
                for (int i = 0; i < fc.bufferNumSamples; ++i)
                {
                    const int r = (pos + size - delaySamples) % size;
                    for (int c = 0; c < chans; ++c)
                    {
                        auto* d = b.getWritePointer (c, fc.bufferStartSample);
                        line.setSample (c, pos, d[i]);
                        d[i] = line.getSample (c, r);
                    }
                    pos = (pos + 1) % size;
                }
            }
            if (gain != 1.0f)
                for (int c = 0; c < chans; ++c) b.applyGain (c, fc.bufferStartSample, fc.bufferNumSamples, gain);
        }

    private:
        juce::AudioBuffer<float> line;
        int pos = 0;
    };

    // Called once per te::Engine, before any song is built.
    inline void registerOurPlugins (te::Engine& engine)
    {
        engine.getPluginManager().createBuiltInType<ClickPlugin>();
        engine.getPluginManager().createBuiltInType<DelayPlugin>();
    }

    // A built-in plugin entry → its plugin, set from the entry (nullptr when it isn't one of ours or Tracktion's).
    inline te::Plugin::Ptr createBuiltin (te::Edit& edit, const juce::var& entry)
    {
        if (entry["kind"].toString() != "builtin") return {};
        auto plugin = edit.getPluginCache().createNewPlugin (entry["name"].toString(), {});
        if (auto* d = dynamic_cast<DelayPlugin*> (plugin.get()))
        {
            d->delaySamples = juce::jmax (0, (int) entry.getProperty ("delaySamples", 0));
            d->reportSamples = juce::jmax (0, (int) entry.getProperty ("reportSamples", 0));
            d->gain = (float) (double) entry.getProperty ("gain", 1.0);
        }
        return plugin;
    }

    // A channel's { "kind": "builtin", "name": … } → that plugin as its instrument (true), or false.
    inline bool insertBuiltinInstrument (te::AudioTrack& track, const juce::var& entry)
    {
        auto plugin = createBuiltin (track.edit, entry);
        if (plugin == nullptr) return false;
        track.pluginList.insertPlugin (plugin, 0, nullptr);
        return true;
    }
}
